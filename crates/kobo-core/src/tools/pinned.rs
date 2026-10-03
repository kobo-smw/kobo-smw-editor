//! The toolchain's programs: where each is, and the builds Kobo pins.
//!
//! A tool is found in this order: a path the user configured
//! ([`config::tool_path`]: an environment variable, then `[tools]` in the
//! config file), then, for the licensed tools (Asar, PIXI, UberASM Tool),
//! the build Kobo pins for the platform. Those are built from source by
//! the companion repository, `kobo-smw/kobo-tools`, and published as a
//! release; `pinned.toml` names the release and each build's SHA-256.
//! The first use of one downloads it, checks its hash, and unpacks it into
//! the user's cache ([`cache_dir`]), where later uses find it without the
//! network. `KOBO_OFFLINE` turns downloads off.
//!
//! A configured path wins, and a build that runs one is no longer a
//! function of Kobo's version and the project alone: [`Located::note`]
//! says so. AddmusicK, GPS, and SA-1 Pack have no licence, so Kobo pins no
//! build of them and they come only from the user.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;
use sha1::Sha1;
use sha2::{Digest, Sha256};

use super::{ASAR_LIBRARY_NAME, ToolError, hash_tree};
use crate::config::{self, Setting};

/// Turns downloads off: a pinned build must already be in the cache.
pub const OFFLINE_ENV_VAR: &str = "KOBO_OFFLINE";
/// Where pinned builds are cached, instead of `kobo/tools` in the user's
/// cache directory.
pub const CACHE_ENV_VAR: &str = "KOBO_TOOL_CACHE";
/// Where to download pinned builds from instead of the release: a folder
/// URL holding the same files, a mirror say. They are checked the same way.
pub const MIRROR_ENV_VAR: &str = "KOBO_TOOL_MIRROR";

/// A program of the toolchain.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tool {
    Asar,
    Pixi,
    UberAsm,
    Gps,
    AddmusicK,
    Sa1Pack,
}

impl Tool {
    pub const ALL: [Tool; 6] = [
        Tool::Asar,
        Tool::Pixi,
        Tool::UberAsm,
        Tool::Gps,
        Tool::AddmusicK,
        Tool::Sa1Pack,
    ];

    /// The tool's name, as its authors write it.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Asar => "Asar",
            Tool::Pixi => "PIXI",
            Tool::UberAsm => "UberASM Tool",
            Tool::Gps => "GPS",
            Tool::AddmusicK => "AddmusicK",
            Tool::Sa1Pack => "SA-1 Pack",
        }
    }

    /// Its key in `[tools]` and in `pinned.toml`.
    pub fn key(self) -> &'static str {
        match self {
            Tool::Asar => "asar",
            Tool::Pixi => "pixi",
            Tool::UberAsm => "uberasm",
            Tool::Gps => "gps",
            Tool::AddmusicK => "addmusick",
            Tool::Sa1Pack => "sa1pack",
        }
    }

    /// The environment variable that overrides it.
    pub fn env_var(self) -> &'static str {
        match self {
            Tool::Asar => config::ASAR_ENV_VAR,
            Tool::Pixi => config::PIXI_ENV_VAR,
            Tool::UberAsm => config::UBERASM_ENV_VAR,
            Tool::Gps => config::GPS_ENV_VAR,
            Tool::AddmusicK => config::ADDMUSICK_ENV_VAR,
            Tool::Sa1Pack => config::SA1PACK_ENV_VAR,
        }
    }

    /// Parses a [`Tool::key`].
    pub fn from_key(key: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.key() == key)
    }

    /// The tool as Kobo pins it, if it does: the licensed ones.
    pub fn pinned(self) -> Option<&'static Pinned> {
        pins().tools.get(self.key())
    }

    /// The pinned build for this platform, if there is one.
    pub fn pinned_build(self) -> Option<&'static PinnedBuild> {
        self.pinned()?.builds.get(platform()?)
    }

    /// The configured path, if any.
    pub fn configured(self) -> Result<Option<(PathBuf, Setting)>, ToolError> {
        Ok(config::tool_path(self.key(), self.env_var())?)
    }

    /// Finds the tool: the configured path, or the pinned build, which is
    /// downloaded into the cache first if it is not there and downloads
    /// are not off ([`OFFLINE_ENV_VAR`]).
    pub fn locate(self) -> Result<Located, ToolError> {
        self.locate_with(!offline())
    }

    /// [`Tool::locate`] without the network: the configured path, or the
    /// pinned build if the cache has it.
    pub fn locate_offline(self) -> Result<Located, ToolError> {
        self.locate_with(false)
    }

    fn locate_with(self, download: bool) -> Result<Located, ToolError> {
        if let Some((path, setting)) = self.configured()? {
            return Ok(Located {
                tool: self,
                path,
                origin: Origin::Configured(setting),
            });
        }
        let Some(pinned) = self.pinned() else {
            return Err(ToolError::NotConfigured {
                tool: self.name(),
                env: self.env_var(),
                key: self.key(),
            });
        };
        let Some(build) = self.pinned_build() else {
            return Err(ToolError::NoBuild {
                tool: self.name(),
                platform: platform().unwrap_or("this platform"),
                env: self.env_var(),
                key: self.key(),
            });
        };
        let cache = cache_dir().ok_or(ToolError::NoCache)?;
        let root = match cached(&cache, build) {
            Some(root) => root,
            None if download => fetch(&format!("{}{}", base_url(), build.file), build, &cache)?,
            None => {
                return Err(ToolError::NotCached {
                    tool: self.name(),
                    version: pinned.version.clone(),
                    cache,
                });
            }
        };
        Ok(Located {
            tool: self,
            path: self.path_in(root),
            origin: Origin::Pinned {
                release: &pins().release,
                sha256: &build.sha256,
            },
        })
    }

    /// Whether [`Tool::locate`] would download: downloads are on, nothing
    /// is configured, and the pinned build is not in the cache.
    pub fn needs_download(self) -> bool {
        !offline()
            && matches!(self.configured(), Ok(None))
            && self
                .pinned_build()
                .zip(cache_dir())
                .is_some_and(|(build, cache)| cached(&cache, build).is_none())
    }

    /// What a build's folder gives for the tool: Asar's library, or the
    /// folder itself for a program run in a copy of its folder.
    fn path_in(self, root: PathBuf) -> PathBuf {
        match self {
            Tool::Asar => root.join(ASAR_LIBRARY_NAME),
            _ => root,
        }
    }
}

impl fmt::Display for Tool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Where a located tool came from.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Origin {
    /// The build Kobo pins, from the release, checked by its hash.
    Pinned {
        release: &'static str,
        sha256: &'static str,
    },
    /// A path the user set.
    Configured(Setting),
}

/// A tool found by [`Tool::locate`]: Asar's library, or a program's
/// folder.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Located {
    pub tool: Tool,
    pub path: PathBuf,
    pub origin: Origin,
}

impl Located {
    /// Whether a build that runs it is a function of Kobo's version and
    /// the project alone: the tool is Kobo's pinned build.
    pub fn reproducible(&self) -> bool {
        matches!(self.origin, Origin::Pinned { .. })
    }

    /// Adds what identifies the tool to a stage's key: the pinned build's
    /// hash, which its files were checked against, or every file of a
    /// configured one.
    pub fn hash_into(&self, hash: &mut Sha1) -> Result<(), ToolError> {
        match &self.origin {
            Origin::Pinned { release, sha256 } => {
                hash.update(self.tool.key());
                hash.update([0]);
                hash.update(release);
                hash.update([0]);
                hash.update(sha256);
                Ok(())
            }
            Origin::Configured(_) => hash_tree(hash, &self.path),
        }
    }

    /// What a build report should say about the tool, if anything: a
    /// configured one makes the build depend on the user's copy.
    pub fn note(&self) -> Option<String> {
        let Origin::Configured(setting) = &self.origin else {
            return None;
        };
        let tool = self.tool.name();
        let path = self.path.display();
        Some(match self.tool.pinned() {
            Some(pinned) => format!(
                "{tool} is {path} ({setting}), not the {} build Kobo pins: this build is not \
                 reproducible elsewhere",
                pinned.version
            ),
            None => format!(
                "{tool} is {path} ({setting}); Kobo pins no build of it (it has no licence), so \
                 this build is repeatable only with the same copy"
            ),
        })
    }
}

/// `pinned.toml`: the kobo-tools release Kobo fetches from.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pins {
    /// The release's tag.
    pub release: String,
    /// Where its files are, ending in `/`.
    pub url: String,
    /// By [`Tool::key`].
    pub tools: std::collections::BTreeMap<String, Pinned>,
}

/// A tool as Kobo pins it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pinned {
    pub name: String,
    pub version: String,
    pub licence: String,
    /// The upstream commit it was built from.
    pub commit: String,
    /// By platform ([`platform`]).
    pub builds: std::collections::BTreeMap<String, PinnedBuild>,
}

/// One platform's build: a `.tar.gz` of one folder.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedBuild {
    pub file: String,
    pub sha256: String,
    pub size: u64,
    /// The folder the archive holds.
    pub root: String,
}

/// The pins Kobo was built with.
pub fn pins() -> &'static Pins {
    static PINS: OnceLock<Pins> = OnceLock::new();
    PINS.get_or_init(|| {
        toml::from_str(include_str!("pinned.toml")).expect("pinned.toml is Kobo's own")
    })
}

/// This platform, as the builds are named, if Kobo pins any for it.
pub fn platform() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-x64")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("windows-x64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos-arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("macos-x64")
    } else {
        None
    }
}

/// The folder URL builds are downloaded from, ending in `/`.
pub fn base_url() -> String {
    match std::env::var(MIRROR_ENV_VAR).ok().filter(|u| !u.is_empty()) {
        Some(url) if url.ends_with('/') => url,
        Some(url) => url + "/",
        None => pins().url.clone(),
    }
}

fn offline() -> bool {
    std::env::var_os(OFFLINE_ENV_VAR).is_some_and(|v| !v.is_empty() && v != "0")
}

/// Where pinned builds are unpacked: `KOBO_TOOL_CACHE`, or `kobo/tools`
/// in the user's cache directory.
pub fn cache_dir() -> Option<PathBuf> {
    match std::env::var_os(CACHE_ENV_VAR).filter(|d| !d.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => dirs::cache_dir().map(|d| d.join("kobo").join("tools")),
    }
}

/// The folder a build unpacks into: named by its hash, so a build is
/// never mistaken for another, and made whole before it is renamed there.
fn entry(cache: &Path, build: &PinnedBuild) -> PathBuf {
    cache.join(&build.sha256[..16.min(build.sha256.len())])
}

/// The build's folder, if the cache has it.
fn cached(cache: &Path, build: &PinnedBuild) -> Option<PathBuf> {
    let root = entry(cache, build).join(&build.root);
    root.is_dir().then_some(root)
}

/// Downloads a build from `url` into `cache`, checks its size and hash,
/// unpacks it, and returns its folder. Nothing is left in the cache if a
/// check fails.
pub(crate) fn fetch(url: &str, build: &PinnedBuild, cache: &Path) -> Result<PathBuf, ToolError> {
    let io_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ToolError::Io { path, source }
    };
    fs::create_dir_all(cache).map_err(io_error(cache))?;
    let partial = cache.join(format!(".partial-{}-{}", std::process::id(), build.root));
    let _ = fs::remove_dir_all(&partial);
    let result = (|| {
        let archive = download(url, build, &partial)?;
        unpack(&archive, &partial, &build.root)?;
        fs::remove_file(&archive).map_err(io_error(&archive))?;
        let target = entry(cache, build);
        if let Err(e) = fs::rename(&partial, &target) {
            // Another process unpacked the same build first.
            if cached(cache, build).is_none() {
                return Err(ToolError::Io {
                    path: target,
                    source: e,
                });
            }
        }
        Ok(cached(cache, build).expect("just unpacked"))
    })();
    let _ = fs::remove_dir_all(&partial);
    result
}

fn download(url: &str, build: &PinnedBuild, dir: &Path) -> Result<PathBuf, ToolError> {
    let failed = |message: String| ToolError::Download {
        url: url.to_owned(),
        message,
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_global(Some(Duration::from_secs(30 * 60)))
        .user_agent(concat!("kobo/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| failed(e.to_string()))?;
    fs::create_dir_all(dir).map_err(|source| ToolError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let path = dir.join(&build.file);
    let mut file = File::create(&path).map_err(|source| ToolError::Io {
        path: path.clone(),
        source,
    })?;
    // Unlimited: reading stops as soon as the file is longer than the pins
    // say, before it is written.
    let mut reader = response.body_mut().as_reader();
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buffer = vec![0; 1 << 16];
    loop {
        let n = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(failed(e.to_string())),
        };
        size += n as u64;
        if size > build.size {
            break;
        }
        hash.update(&buffer[..n]);
        file.write_all(&buffer[..n])
            .map_err(|source| ToolError::Io {
                path: path.clone(),
                source,
            })?;
    }
    let found: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if size != build.size || found != build.sha256 {
        return Err(ToolError::Checksum {
            url: url.to_owned(),
            expected: format!("{} bytes, sha256 {}", build.size, build.sha256),
            found: if size > build.size {
                format!("more than {} bytes", build.size)
            } else {
                format!("{size} bytes, sha256 {found}")
            },
        });
    }
    Ok(path)
}

/// Unpacks a build into `dir`: every entry a file or folder under `root`.
fn unpack(archive: &Path, dir: &Path, root: &str) -> Result<(), ToolError> {
    let bad = |message: String| ToolError::Archive {
        path: archive.to_path_buf(),
        message,
    };
    let file = File::open(archive).map_err(|source| ToolError::Io {
        path: archive.to_path_buf(),
        source,
    })?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in tar.entries().map_err(|e| bad(e.to_string()))? {
        let mut entry = entry.map_err(|e| bad(e.to_string()))?;
        let path = entry.path().map_err(|e| bad(e.to_string()))?.into_owned();
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            return Err(bad(format!("{} is not a file or folder", path.display())));
        }
        let inside = path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
            && path.starts_with(root);
        if !inside {
            return Err(bad(format!("{} is outside {root}", path.display())));
        }
        // `unpack_in` refuses anything that would land outside `dir`.
        entry.unpack_in(dir).map_err(|e| bad(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    #[test]
    fn the_pins_name_every_licensed_tool_for_every_platform() {
        let pins = pins();
        assert!(
            pins.url
                .starts_with("https://github.com/kobo-smw/kobo-tools/releases/download/")
        );
        assert!(pins.url.ends_with(&format!("{}/", pins.release)));
        for tool in [Tool::Asar, Tool::Pixi, Tool::UberAsm] {
            let pinned = tool.pinned().unwrap();
            assert_eq!(pinned.name, tool.name());
            assert_eq!(pinned.commit.len(), 40);
            for platform in ["linux-x64", "windows-x64", "macos-arm64", "macos-x64"] {
                let build = &pinned.builds[platform];
                assert_eq!(build.sha256.len(), 64, "{tool} {platform}");
                assert!(build.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
                assert_eq!(
                    build.root,
                    format!("{}-{}-{platform}", tool.key(), pinned.version)
                );
                assert_eq!(build.file, format!("{}.tar.gz", build.root));
            }
        }
        // Those without a licence are never pinned.
        for tool in [Tool::Gps, Tool::AddmusicK, Tool::Sa1Pack] {
            assert!(tool.pinned().is_none(), "{tool}");
        }
        assert_eq!(Tool::Asar.pinned().unwrap().version, "1.91");
    }

    /// A `.tar.gz` of `entries` (path, contents), as kobo-tools makes them.
    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, path, *data).unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar.into_inner().unwrap()).unwrap();
        gz.finish().unwrap()
    }

    fn build_of(bytes: &[u8], root: &str) -> PinnedBuild {
        PinnedBuild {
            file: format!("{root}.tar.gz"),
            sha256: Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            size: bytes.len() as u64,
            root: root.to_owned(),
        }
    }

    /// Serves `body` to each of `count` requests on a local port, and
    /// returns the URL.
    fn serve(body: Vec<u8>, count: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/tool.tar.gz", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().take(count) {
                let mut stream = stream.unwrap();
                let mut reader = io::BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap() > 2 {
                    line.clear();
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        url
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kobo-fetch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_build_is_downloaded_checked_and_unpacked_once() {
        let bytes = archive(&[
            ("tool-1-x/run", b"#!/bin/sh\n"),
            ("tool-1-x/data/a.txt", b"a"),
        ]);
        let build = build_of(&bytes, "tool-1-x");
        let cache = temp("ok");
        let url = serve(bytes, 1);
        let root = fetch(&url, &build, &cache).unwrap();
        assert_eq!(root, cached(&cache, &build).unwrap());
        assert_eq!(fs::read(root.join("data/a.txt")).unwrap(), b"a");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(root.join("run")).unwrap().permissions().mode();
            assert!(mode & 0o100 != 0, "the program is executable");
        }
        // Nothing but the build's folder is left.
        let names: Vec<_> = fs::read_dir(&cache)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1);
        let _ = fs::remove_dir_all(&cache);
    }

    #[test]
    fn a_build_that_fails_its_hash_is_not_kept() {
        let bytes = archive(&[("tool-1-x/run", b"run")]);
        let mut build = build_of(&bytes, "tool-1-x");
        build.sha256 = "0".repeat(64);
        let cache = temp("hash");
        let error = fetch(&serve(bytes.clone(), 1), &build, &cache).unwrap_err();
        assert!(matches!(error, ToolError::Checksum { .. }), "{error}");
        assert!(cached(&cache, &build).is_none());
        assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);

        // A longer file than the pins say is refused before it is read whole.
        let mut build = build_of(&bytes, "tool-1-x");
        build.size -= 1;
        let error = fetch(&serve(bytes, 1), &build, &cache).unwrap_err();
        assert!(error.to_string().contains("more than"), "{error}");
        let _ = fs::remove_dir_all(&cache);
    }

    #[test]
    fn an_archive_may_not_reach_outside_its_folder() {
        for path in ["other/run", "tool-1-x/../escape"] {
            let mut tar = tar::Builder::new(Vec::new());
            let mut header = tar::Header::new_gnu();
            header.set_size(1);
            header.set_mode(0o644);
            // Written raw: the builder itself refuses `..`.
            header.as_gnu_mut().unwrap().name[..path.len()].copy_from_slice(path.as_bytes());
            header.set_cksum();
            tar.append(&header, &b"x"[..]).unwrap();
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            gz.write_all(&tar.into_inner().unwrap()).unwrap();
            let bytes = gz.finish().unwrap();
            let build = build_of(&bytes, "tool-1-x");
            let cache = temp("escape");
            let error = fetch(&serve(bytes, 1), &build, &cache).unwrap_err();
            assert!(
                matches!(error, ToolError::Archive { .. }),
                "{path}: {error}"
            );
            assert!(!cache.join("escape").exists());
            assert_eq!(fs::read_dir(&cache).unwrap().count(), 0, "{path}");
            let _ = fs::remove_dir_all(&cache);
        }
    }

    #[test]
    fn a_failed_download_says_where_from() {
        // Nothing listens on a port just freed.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url = format!("http://127.0.0.1:{port}/tool.tar.gz");
        let build = build_of(b"", "tool-1-x");
        let cache = temp("refused");
        let error = fetch(&url, &build, &cache).unwrap_err();
        assert!(matches!(error, ToolError::Download { .. }), "{error}");
        assert!(error.to_string().contains(&url), "{error}");
        let _ = fs::remove_dir_all(&cache);
    }
}
