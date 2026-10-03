//! Asar, through its shared library.
//!
//! Asar 1.91 is LGPL-3.0, so Kobo loads `libasar` (`asar.dll` on Windows)
//! at run time rather than linking it. [`Asar::load`] takes the library's
//! path and [`Asar::configured`] finds it through [`Tool::locate`]: the
//! path the user configured, or the build Kobo pins, fetched on first use.
//! Both check the library's API version.
//! The library keeps its state in globals and is not thread-safe, so every
//! call into it holds one process-wide lock, and each patch starts from a
//! reset.
//!
//! [`Asar::patch`] applies a [`Patch`] to a copy of a [`Rom`] in memory,
//! leaving the checksum alone for the build to fix, and returns the new
//! image with what Asar reported. It fails if the patch damaged a RATS
//! block that was there before it ([`Snapshot::check`]), which Asar 1.91's
//! free-space search can do (`docs/toolchain.md`).
//!
//! The library trusts its caller at the boundary, so Kobo checks there:
//! an image longer than [`BUFFER_LEN`] is refused before the call, since
//! Asar copies it into a buffer of its own without checking, and the
//! result is read as the headerless image it is, padded to a whole bank,
//! rather than through copier-header detection, which a growth of 512
//! bytes would trip.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;

use libloading::Library;
use thiserror::Error;

use crate::addr::{Mapping, SnesAddr};
use crate::config::ConfigError;
use crate::rats::{Damage, Snapshot};
use crate::rom::{BANK_LEN, Rom, RomError};
use crate::tools::{Tool, ToolError};

/// The API version Kobo is written against, Asar 1.91's. A library with a
/// later minor version is compatible; another major version is not.
pub const API_VERSION: i32 = 303;

/// The Asar release a build pins, 1.91.
pub const PINNED: Version = Version {
    major: 1,
    minor: 9,
    bugfix: 1,
};

/// The buffer Asar patches in, and the longest image [`Asar::patch`]
/// takes: room for the largest image any mapping addresses, so a patch
/// can expand the image or switch it to SA-1. Asar copies the image into
/// a 16 MiB buffer of its own without checking `buflen`, so this must
/// never exceed that.
pub const BUFFER_LEN: usize = Mapping::BigSa1Rom.max_rom_len();

/// The stack a patch runs on, as Asar's command-line tool gets on Linux
/// and macOS; its own library gives it 4 MiB on Windows.
const STACK_LEN: usize = 8 << 20;

/// Serialises every call into any loaded Asar.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Error)]
pub enum AsarError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Locate(#[from] Box<ToolError>),
    #[error("failed to load Asar from {path}: {source}")]
    Load {
        path: PathBuf,
        #[source]
        source: libloading::Error,
    },
    #[error("{path} is not Asar's library: {source}")]
    Symbol {
        path: PathBuf,
        #[source]
        source: libloading::Error,
    },
    #[error("{path} has Asar API version {found}; Kobo needs {API_VERSION} or a later 3.x")]
    ApiVersion { path: PathBuf, found: i32 },
    #[error("{path} refused to initialise")]
    Init { path: PathBuf },
    #[error("{0:?} cannot be passed to Asar")]
    BadString(String),
    #[error("{}", lines(errors))]
    Failed {
        errors: Vec<Message>,
        warnings: Vec<Message>,
        prints: Vec<String>,
    },
    #[error("a {len}-byte image is more than the {BUFFER_LEN}-byte buffer Asar patches in")]
    InputTooLarge { len: usize },
    #[error("the patch left a {len}-byte image, more than {mapping:?} addresses")]
    TooLarge { len: usize, mapping: Mapping },
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error(
        "the patch damaged RATS blocks that were there before it:\n{}",
        lines(damage)
    )]
    Damaged { damage: Vec<Damage>, output: Output },
}

fn lines(items: &[impl fmt::Display]) -> String {
    let lines: Vec<String> = items.iter().map(ToString::to_string).collect();
    lines.join("\n")
}

/// An Asar version. Asar writes it as its tool does: 1.9.1 is "1.91".
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub bugfix: u32,
}

impl Version {
    /// From Asar's `major * 10000 + minor * 100 + bugfix`.
    fn from_int(v: u32) -> Self {
        Self {
            major: v / 10000,
            minor: v / 100 % 100,
            bugfix: v % 100,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dot = if self.minor >= 10 || self.bugfix >= 10 {
            "."
        } else {
            ""
        };
        write!(f, "{}.{}{dot}{}", self.major, self.minor, self.bugfix)
    }
}

/// A patch to apply: its main file, where its includes are found, and the
/// defines it is given.
#[derive(Clone, Debug)]
pub struct Patch {
    main: PathBuf,
    include_paths: Vec<PathBuf>,
    defines: Vec<(String, String)>,
    files: Vec<(PathBuf, Vec<u8>)>,
    warnings: Vec<(String, bool)>,
}

impl Patch {
    /// The patch whose main file is at `main`. Relative paths are taken
    /// from the current directory.
    pub fn new(main: impl Into<PathBuf>) -> Self {
        Self {
            main: main.into(),
            include_paths: Vec::new(),
            defines: Vec::new(),
            files: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// A patch whose main file is `source`, held in memory as if at
    /// `path`, which its relative includes are resolved against.
    pub fn source(path: impl Into<PathBuf>, source: impl Into<String>) -> Self {
        let path = path.into();
        Self::new(path.clone()).file(path, source.into())
    }

    /// Another directory to look for included files in, after the
    /// including file's own.
    pub fn include_path(mut self, dir: impl Into<PathBuf>) -> Self {
        self.include_paths.push(dir.into());
        self
    }

    /// Defines `!name` as `value`.
    pub fn define(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.defines.push((name.into(), value.into()));
        self
    }

    /// A file held in memory as if at `path`, which takes the place of
    /// anything on disk there.
    pub fn file(mut self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) -> Self {
        self.files.push((path.into(), contents.into()));
        self
    }

    /// Turns an Asar warning (`W` and its name, as `Wfreespace_leaked`)
    /// on or off.
    pub fn warning(mut self, id: impl Into<String>, enabled: bool) -> Self {
        self.warnings.push((id.into(), enabled));
        self
    }
}

/// A warning or error, as Asar reports it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Message {
    /// The whole message, with its file, line, and source line.
    pub text: String,
    /// The message alone.
    pub raw: String,
    /// The file it is about, if any.
    pub file: Option<String>,
    /// Its line in that file, counting from 1.
    pub line: Option<u32>,
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Label {
    pub name: String,
    pub addr: SnesAddr,
}

/// What a patch reported besides its errors.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Output {
    pub warnings: Vec<Message>,
    /// What `print` printed, in order.
    pub prints: Vec<String>,
    /// Every label, by name.
    pub labels: Vec<Label>,
    /// The file ranges the patch wrote, erasures included.
    pub written: Vec<Range<usize>>,
}

impl Output {
    pub fn label(&self, name: &str) -> Option<SnesAddr> {
        self.labels.iter().find(|l| l.name == name).map(|l| l.addr)
    }
}

/// A patched image and what Asar reported.
#[derive(Debug)]
pub struct Patched {
    pub rom: Rom,
    pub output: Output,
}

mod ffi {
    use std::ffi::{c_char, c_int, c_void};

    #[repr(C)]
    pub struct ErrorData {
        pub fullerrdata: *const c_char,
        pub rawerrdata: *const c_char,
        pub block: *const c_char,
        pub filename: *const c_char,
        pub line: c_int,
        pub callerfilename: *const c_char,
        pub callerline: c_int,
        pub errid: c_int,
    }

    #[repr(C)]
    pub struct LabelData {
        pub name: *const c_char,
        pub location: c_int,
    }

    #[repr(C)]
    pub struct DefineData {
        pub name: *const c_char,
        pub contents: *const c_char,
    }

    #[repr(C)]
    pub struct WrittenBlockData {
        pub pcoffset: c_int,
        pub snesoffset: c_int,
        pub numbytes: c_int,
    }

    #[repr(C)]
    pub struct WarnSetting {
        pub warnid: *const c_char,
        pub enabled: bool,
    }

    #[repr(C)]
    pub struct MemoryFile {
        pub path: *const c_char,
        pub buffer: *const c_void,
        pub length: usize,
    }

    /// `patchparams` of API version 3.03.
    #[repr(C)]
    pub struct PatchParams {
        pub structsize: c_int,
        pub patchloc: *const c_char,
        pub romdata: *mut c_char,
        pub buflen: c_int,
        pub romlen: *mut c_int,
        pub includepaths: *const *const c_char,
        pub numincludepaths: c_int,
        pub should_reset: bool,
        pub additional_defines: *const DefineData,
        pub additional_define_count: c_int,
        pub stdincludesfile: *const c_char,
        pub stddefinesfile: *const c_char,
        pub warning_settings: *const WarnSetting,
        pub warning_setting_count: c_int,
        pub memory_files: *const MemoryFile,
        pub memory_file_count: c_int,
        pub override_checksum_gen: bool,
        pub generate_checksum: bool,
    }

    pub type Count<T> = unsafe extern "C" fn(*mut c_int) -> *const T;
}

/// The library's functions Kobo calls.
struct Api {
    version: unsafe extern "C" fn() -> c_int,
    apiversion: unsafe extern "C" fn() -> c_int,
    init: unsafe extern "C" fn() -> bool,
    reset: unsafe extern "C" fn() -> bool,
    close: unsafe extern "C" fn(),
    patch_ex: unsafe extern "C" fn(*const ffi::PatchParams) -> bool,
    geterrors: ffi::Count<ffi::ErrorData>,
    getwarnings: ffi::Count<ffi::ErrorData>,
    getprints: ffi::Count<*const c_char>,
    getalllabels: ffi::Count<ffi::LabelData>,
    getwrittenblocks: ffi::Count<ffi::WrittenBlockData>,
}

/// A loaded Asar library.
pub struct Asar {
    api: Api,
    path: PathBuf,
    version: Version,
    // Dropped last: the function pointers above point into it.
    _library: Library,
}

impl From<ToolError> for AsarError {
    fn from(e: ToolError) -> Self {
        AsarError::Locate(Box::new(e))
    }
}

impl fmt::Debug for Asar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Asar")
            .field("path", &self.path)
            .field("version", &self.version)
            .finish()
    }
}

impl Asar {
    /// Loads the library the user configured, or the one Kobo pins,
    /// downloading it on first use ([`Tool::locate`]).
    pub fn configured() -> Result<Self, AsarError> {
        Self::load(Tool::Asar.locate()?.path)
    }

    /// [`Asar::configured`] without the network: the configured library,
    /// or the pinned one if the cache has it already.
    pub fn offline() -> Result<Self, AsarError> {
        Self::load(Tool::Asar.locate_offline()?.path)
    }

    /// Loads the library at `path` and checks its API version.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, AsarError> {
        let path = path.as_ref().to_path_buf();
        let _guard = lock();
        // SAFETY: loading Asar runs only its C++ static initialisers.
        let library = unsafe { Library::new(&path) }.map_err(|source| AsarError::Load {
            path: path.clone(),
            source,
        })?;
        // SAFETY: each field's type is the function's in Asar's `asar.h`
        // for API version 3.03, and the pointers live as long as
        // `library`, which `Asar` keeps.
        let api = unsafe {
            let l = (&library, path.as_path());
            Api {
                version: symbol(l, "asar_version")?,
                apiversion: symbol(l, "asar_apiversion")?,
                init: symbol(l, "asar_init")?,
                reset: symbol(l, "asar_reset")?,
                close: symbol(l, "asar_close")?,
                patch_ex: symbol(l, "asar_patch_ex")?,
                geterrors: symbol(l, "asar_geterrors")?,
                getwarnings: symbol(l, "asar_getwarnings")?,
                getprints: symbol(l, "asar_getprints")?,
                getalllabels: symbol(l, "asar_getalllabels")?,
                getwrittenblocks: symbol(l, "asar_getwrittenblocks")?,
            }
        };
        // SAFETY: these take no arguments. Asking for the API version is
        // what lets `asar_init` succeed.
        let found = unsafe { (api.apiversion)() };
        if found < API_VERSION || found / 100 != API_VERSION / 100 {
            return Err(AsarError::ApiVersion { path, found });
        }
        // SAFETY: as above.
        if !unsafe { (api.init)() } {
            return Err(AsarError::Init { path });
        }
        // SAFETY: as above.
        let version = Version::from_int(unsafe { (api.version)() }.max(0) as u32);
        Ok(Self {
            api,
            path,
            version,
            _library: library,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn version(&self) -> Version {
        self.version
    }

    /// Applies `patch` to a copy of `rom`. The checksum is left as it was;
    /// the image may grow, to a whole number of banks, and its mapping is
    /// the one [`Rom::from_headerless`] reads from the result. Fails with
    /// [`AsarError::Failed`] if Asar reports errors, and with
    /// [`AsarError::Damaged`] if the patch damaged a RATS block that `rom`
    /// had.
    pub fn patch(&self, rom: &Rom, patch: &Patch) -> Result<Patched, AsarError> {
        let before = Snapshot::take(rom);
        let (mut image, output) = self.run(rom.data(), patch)?;
        // Asar leaves the image as long as its last write reached.
        image.resize(image.len().next_multiple_of(BANK_LEN), 0);
        let rom = Rom::from_headerless(image)?;
        let mapping = rom.mapping();
        if rom.len() > mapping.max_rom_len() {
            let len = rom.len();
            return Err(AsarError::TooLarge { len, mapping });
        }
        let damage = before.check(&rom, &output.written);
        if !damage.is_empty() {
            return Err(AsarError::Damaged { damage, output });
        }
        Ok(Patched { rom, output })
    }

    /// Runs Asar on `image`, returning the patched image and the output.
    fn run(&self, image: &[u8], patch: &Patch) -> Result<(Vec<u8>, Output), AsarError> {
        if image.len() > BUFFER_LEN {
            return Err(AsarError::InputTooLarge { len: image.len() });
        }
        let main = c_path(&patch.main)?;
        let include_paths = patch
            .include_paths
            .iter()
            .map(|p| c_path(p))
            .collect::<Result<Vec<_>, _>>()?;
        let defines = patch
            .defines
            .iter()
            .map(|(name, value)| Ok((c_string(name)?, c_string(value)?)))
            .collect::<Result<Vec<_>, AsarError>>()?;
        let files = patch
            .files
            .iter()
            .map(|(path, contents)| Ok((c_path(path)?, contents.as_slice())))
            .collect::<Result<Vec<_>, AsarError>>()?;
        let warnings = patch
            .warnings
            .iter()
            .map(|(id, enabled)| Ok((c_string(id)?, *enabled)))
            .collect::<Result<Vec<_>, AsarError>>()?;
        let mut buffer = image.to_vec();
        buffer.resize(BUFFER_LEN, 0);

        let _guard = lock();
        let run = || {
            let include_ptrs: Vec<*const c_char> =
                include_paths.iter().map(|p| p.as_ptr()).collect();
            let define_data: Vec<ffi::DefineData> = defines
                .iter()
                .map(|(name, value)| ffi::DefineData {
                    name: name.as_ptr(),
                    contents: value.as_ptr(),
                })
                .collect();
            let file_data: Vec<ffi::MemoryFile> = files
                .iter()
                .map(|(path, contents)| ffi::MemoryFile {
                    path: path.as_ptr(),
                    buffer: contents.as_ptr().cast::<c_void>(),
                    length: contents.len(),
                })
                .collect();
            let warning_data: Vec<ffi::WarnSetting> = warnings
                .iter()
                .map(|(id, enabled)| ffi::WarnSetting {
                    warnid: id.as_ptr(),
                    enabled: *enabled,
                })
                .collect();
            let mut romlen = image.len() as c_int;
            let params = ffi::PatchParams {
                structsize: size_of::<ffi::PatchParams>() as c_int,
                patchloc: main.as_ptr(),
                romdata: buffer.as_mut_ptr().cast::<c_char>(),
                buflen: buffer.len() as c_int,
                romlen: &mut romlen,
                includepaths: include_ptrs.as_ptr(),
                numincludepaths: include_ptrs.len() as c_int,
                should_reset: true,
                additional_defines: define_data.as_ptr(),
                additional_define_count: define_data.len() as c_int,
                stdincludesfile: ptr::null(),
                stddefinesfile: ptr::null(),
                warning_settings: warning_data.as_ptr(),
                warning_setting_count: warning_data.len() as c_int,
                memory_files: file_data.as_ptr(),
                memory_file_count: file_data.len() as c_int,
                override_checksum_gen: true,
                generate_checksum: false,
            };
            // SAFETY: every pointer in `params` is to data that outlives
            // the call, `romdata` has `buflen` bytes, and the lock is held.
            let ok = unsafe { (self.api.patch_ex)(&params) };
            // SAFETY: the lock is held, and everything the lists point to
            // is copied out before `reset` frees it.
            let (errors, output) = unsafe { self.results() };
            // SAFETY: as above.
            unsafe { (self.api.reset)() };
            (ok, romlen, errors, output)
        };
        let (ok, romlen, errors, output) = thread::scope(|scope| {
            thread::Builder::new()
                .name("asar".into())
                .stack_size(STACK_LEN)
                .spawn_scoped(scope, run)
                .expect("a thread for Asar")
                .join()
                .expect("Asar's thread does not panic")
        });
        if !ok || !errors.is_empty() {
            return Err(AsarError::Failed {
                errors,
                warnings: output.warnings,
                prints: output.prints,
            });
        }
        buffer.truncate(romlen.max(0) as usize);
        Ok((buffer, output))
    }

    /// Copies the errors and the output of the last patch.
    ///
    /// # Safety
    ///
    /// The caller holds the lock.
    unsafe fn results(&self) -> (Vec<Message>, Output) {
        // SAFETY: each list is `count` entries that stay valid until the
        // next call into Asar, which the caller's lock holds off.
        unsafe {
            let errors = list(self.api.geterrors)
                .iter()
                .map(|e| message(e))
                .collect();
            let warnings = list(self.api.getwarnings)
                .iter()
                .map(|e| message(e))
                .collect();
            let prints = list(self.api.getprints)
                .iter()
                .map(|&p| text(p).unwrap_or_default())
                .collect();
            let mut labels: Vec<Label> = list(self.api.getalllabels)
                .iter()
                .map(|l| Label {
                    name: text(l.name).unwrap_or_default(),
                    addr: SnesAddr::new(l.location as u32 & 0xFF_FFFF),
                })
                .collect();
            labels.sort_by(|a, b| a.name.cmp(&b.name));
            let written = list(self.api.getwrittenblocks)
                .iter()
                .map(|w| {
                    let start = w.pcoffset.max(0) as usize;
                    start..start + w.numbytes.max(0) as usize
                })
                .collect();
            let output = Output {
                warnings,
                prints,
                labels,
                written,
            };
            (errors, output)
        }
    }
}

impl Drop for Asar {
    fn drop(&mut self) {
        let _guard = lock();
        // SAFETY: frees what the last patch left; the lock is held.
        unsafe { (self.api.close)() };
    }
}

/// The function `name` in `library`, whose path is given for errors.
///
/// # Safety
///
/// `T` is the function's type.
unsafe fn symbol<T: Copy>((library, path): (&Library, &Path), name: &str) -> Result<T, AsarError> {
    // SAFETY: per the caller.
    let symbol = unsafe { library.get::<T>(name) };
    symbol.map(|s| *s).map_err(|source| AsarError::Symbol {
        path: path.to_path_buf(),
        source,
    })
}

/// The entries of one of Asar's lists.
///
/// # Safety
///
/// `get` is one of Asar's list functions, called under the lock; the slice
/// is valid until the next call into Asar.
unsafe fn list<'a, T>(get: ffi::Count<T>) -> &'a [T] {
    let mut count: c_int = 0;
    // SAFETY: per the caller.
    let items = unsafe { get(&mut count) };
    if items.is_null() || count <= 0 {
        return &[];
    }
    // SAFETY: Asar returns `count` entries.
    unsafe { std::slice::from_raw_parts(items, count as usize) }
}

/// # Safety
///
/// `p` is null or a NUL-terminated string.
unsafe fn text(p: *const c_char) -> Option<String> {
    // SAFETY: per the caller.
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

/// # Safety
///
/// As for [`text`], for each of the strings.
unsafe fn message(e: &ffi::ErrorData) -> Message {
    // SAFETY: per the caller.
    unsafe {
        Message {
            text: text(e.fullerrdata).unwrap_or_default(),
            raw: text(e.rawerrdata).unwrap_or_default(),
            file: text(e.filename).filter(|f| !f.is_empty()),
            line: u32::try_from(e.line).ok().map(|l| l + 1),
        }
    }
}

fn c_string(s: &str) -> Result<CString, AsarError> {
    CString::new(s).map_err(|_| AsarError::BadString(s.to_owned()))
}

/// An absolute path as UTF-8, which is what Asar takes on every platform.
fn c_path(path: &Path) -> Result<CString, AsarError> {
    let bad = || AsarError::BadString(path.display().to_string());
    let absolute = std::path::absolute(path).map_err(|_| bad())?;
    c_string(absolute.to_str().ok_or_else(bad)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_read_as_asar_writes_them() {
        assert_eq!(Version::from_int(10901), PINNED);
        assert_eq!(PINNED.to_string(), "1.91");
        assert_eq!(Version::from_int(10234).to_string(), "1.2.34");
    }

    #[test]
    fn a_missing_library_is_an_error() {
        let path = std::env::temp_dir().join("kobo-no-such-dir/libasar.so");
        assert!(matches!(
            Asar::load(&path),
            Err(AsarError::Load { path: p, .. }) if p == path
        ));
    }

    #[test]
    fn strings_with_nul_bytes_are_refused() {
        assert!(matches!(c_string("a\0b"), Err(AsarError::BadString(_))));
    }
}
