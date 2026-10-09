//! Watching a build while it plays in an emulator: Mesen 2 runs a Lua
//! script Kobo writes (`mesen_script`), which after every frame writes the
//! RAM watched to a report file beside the ROM, and pauses the game, as its
//! debugger's break does, when a watched variable is written to and the
//! watch says to, or when code at a [`Breakpoint`] runs. [`read_report`]
//! reads the file back.
//!
//! The script reports RAM addresses and values, and the breakpoint that
//! stopped the game (an address the user chose, from a build's labels),
//! never where the game's or a patch's code is otherwise, so nothing in it
//! is the clean room's to withhold (`clean_room`). Mesen runs a script given after the ROM on its
//! command line; the script writes its report with Lua's `io`, which Mesen
//! allows with its script window's "Allow access to I/O and OS functions".

use std::path::{Path, PathBuf};

use crate::ram::{RamAddr, RamMap};

/// A variable watched: its name, its vanilla RAM address (resolved through
/// the build's [`RamMap`]), its size (1 or 2 bytes), and whether a write to
/// it pauses the game.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Watch {
    pub name: String,
    pub addr: RamAddr,
    pub size: u8,
    pub pause_on_write: bool,
}

/// Code to stop at: a name, and its address on the bus. On an SA-1
/// cartridge either processor running it stops the game.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Breakpoint {
    pub name: String,
    pub addr: crate::addr::SnesAddr,
}

/// What the script last wrote: the frame, each watch's value in order, the
/// write that paused the game, if one did, and the breakpoint that stopped
/// it (its index, and the frame), if one did.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Report {
    pub frame: u64,
    pub values: Vec<u16>,
    pub paused: Option<Paused>,
    pub stopped: Option<(usize, u64)>,
}

/// A watched write that paused the game: its offset in its memory (work RAM,
/// I-RAM, or BW-RAM), the value, and the frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Paused {
    pub addr: u32,
    pub value: u8,
    pub frame: u64,
}

/// Whether `emulator` is Mesen, which runs Kobo's script: its file name.
pub fn is_mesen(emulator: &Path) -> bool {
    emulator
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.to_ascii_lowercase().contains("mesen"))
}

/// Where the script and its report go: beside the ROM.
pub fn script_path(rom: &Path) -> PathBuf {
    rom.with_extension("kobo-watch.lua")
}

pub fn report_path(rom: &Path) -> PathBuf {
    rom.with_extension("kobo-watch.txt")
}

/// A Lua string literal of `text`.
fn lua_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out += "\\\\",
            '"' => out += "\\\"",
            '\n' => out += "\\n",
            c => out.push(c),
        }
    }
    out + "\""
}

/// The script for Mesen 2 that watches `watches` and stops at `breaks` in
/// a build whose RAM is laid out as `map` says, and writes its report to
/// `report`.
pub fn mesen_script(
    watches: &[Watch],
    breaks: &[Breakpoint],
    map: RamMap,
    report: &Path,
) -> String {
    let entries: Vec<String> = watches
        .iter()
        .map(|w| {
            format!(
                "  {{ 0x{:06X}, {} }},",
                map.resolve(w.addr),
                w.size.clamp(1, 2)
            )
        })
        .collect();
    // A write is caught in its memory's own addresses, as the game writes
    // a variable through any of its mirrors (the direct page, bank $00).
    let pauses: Vec<String> = watches
        .iter()
        .filter(|w| w.pause_on_write)
        .filter_map(|w| {
            let (memory, offset) = memory_of(map.resolve(w.addr))?;
            Some(format!("  {{ emu.memType.{memory}, 0x{offset:05X} }},"))
        })
        .collect();
    let cpus = match map {
        RamMap::Vanilla => "emu.cpuType.snes",
        RamMap::Sa1Pack => "emu.cpuType.snes, emu.cpuType.sa1",
    };
    // Code is caught on each processor's own bus.
    let code = match map {
        RamMap::Vanilla => "{ emu.cpuType.snes, emu.memType.snesMemory }",
        RamMap::Sa1Pack => {
            "{ emu.cpuType.snes, emu.memType.snesMemory }, { emu.cpuType.sa1, emu.memType.sa1Memory }"
        }
    };
    let breaks: Vec<String> = breaks
        .iter()
        .map(|b| format!("  0x{:06X},", b.addr.raw()))
        .collect();
    format!(
        r#"-- Kobo's watch of a build (kobo_core::emulator): after every frame,
-- the RAM watched into the report file; a watched write pauses the game.
local report = {report}
local watches = {{
{entries}
}}
local pauses = {{
{pauses}
}}
local cpus = {{ {cpus} }}
local breaks = {{
{breaks}
}}
local code = {{ {code} }}
local frame = 0
local paused = ""
local stopped = ""

local function value(w)
  local low = emu.read(w[1], emu.memType.snesMemory, false)
  if w[2] == 2 then
    return low + 256 * emu.read(w[1] + 1, emu.memType.snesMemory, false)
  end
  return low
end

local function write_report()
  local f = io.open(report, "w")
  if f == nil then return end
  f:write("frame " .. frame .. "\n")
  local values = {{}}
  for i, w in ipairs(watches) do
    values[i] = string.format("%X", value(w))
  end
  f:write(table.concat(values, " ") .. "\n")
  if paused ~= "" then f:write(paused .. "\n") end
  if stopped ~= "" then f:write(stopped .. "\n") end
  f:close()
end

emu.addEventCallback(function()
  frame = frame + 1
  write_report()
end, emu.eventType.endFrame)

for _, p in ipairs(pauses) do
  for _, cpu in ipairs(cpus) do
    emu.addMemoryCallback(function(address, written)
      paused = string.format("paused %X %02X %d", p[2], written, frame)
      write_report()
      emu.breakExecution()
    end, emu.callbackType.write, p[2], p[2], cpu, p[1])
  end
end

for i, at in ipairs(breaks) do
  for _, c in ipairs(code) do
    emu.addMemoryCallback(function()
      stopped = string.format("stopped %d %d", i - 1, frame)
      write_report()
      emu.breakExecution()
    end, emu.callbackType.exec, at, at, c[1], c[2])
  end
end
"#,
        report = lua_string(&report.to_string_lossy()),
        cpus = cpus,
        entries = entries.join("\n"),
        pauses = pauses.join("\n"),
        breaks = breaks.join("\n"),
        code = code,
    )
}

/// A bus address's memory, as Mesen names it, and its offset there: work
/// RAM, the SA-1's I-RAM, or BW-RAM (Mesen's save RAM).
fn memory_of(bus: u32) -> Option<(&'static str, u32)> {
    match bus {
        0x7E_0000..=0x7F_FFFF => Some(("snesWorkRam", bus - 0x7E_0000)),
        0x00_3000..=0x00_37FF => Some(("sa1InternalRam", bus - 0x3000)),
        0x40_0000..=0x41_FFFF => Some(("snesSaveRam", bus - 0x40_0000)),
        _ => None,
    }
}

/// The report's text read back; `None` for a report not yet whole.
pub fn read_report(text: &str) -> Option<Report> {
    let mut lines = text.lines();
    let frame = lines.next()?.strip_prefix("frame ")?.trim().parse().ok()?;
    let values = lines
        .next()?
        .split_whitespace()
        .map(|v| u16::from_str_radix(v, 16).ok())
        .collect::<Option<Vec<u16>>>()?;
    let (mut paused, mut stopped) = (None, None);
    for line in lines {
        if let Some(rest) = line.strip_prefix("paused ") {
            let mut parts = rest.split_whitespace();
            paused = Some(Paused {
                addr: u32::from_str_radix(parts.next()?, 16).ok()?,
                value: u8::from_str_radix(parts.next()?, 16).ok()?,
                frame: parts.next()?.parse().ok()?,
            });
        } else {
            let mut parts = line.strip_prefix("stopped ")?.split_whitespace();
            stopped = Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?));
        }
    }
    Some(Report {
        frame,
        values,
        paused,
        stopped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesen_is_told_by_its_name() {
        assert!(is_mesen(Path::new("/opt/mesen2/Mesen")));
        assert!(is_mesen(Path::new("C:/Emu/Mesen.exe")));
        assert!(!is_mesen(Path::new("/usr/bin/bsnes")));
    }

    #[test]
    fn the_script_names_each_watch_where_the_build_has_it() {
        let watches = [
            Watch {
                name: "Player x".into(),
                addr: RamAddr::new(0x7E_0094),
                size: 2,
                pause_on_write: false,
            },
            Watch {
                name: "Power-up".into(),
                addr: RamAddr::new(0x7E_0019),
                size: 1,
                pause_on_write: true,
            },
        ];
        let breaks = [Breakpoint {
            name: "kobo_overworld_save_check".into(),
            addr: crate::addr::SnesAddr::new(0x10_8123),
        }];
        let script = mesen_script(
            &watches,
            &breaks,
            RamMap::Vanilla,
            Path::new("/tmp/a \"b\".txt"),
        );
        assert!(
            script.contains("local breaks = {\n  0x108123,\n}"),
            "{script}"
        );
        assert!(script.contains("emu.callbackType.exec"), "{script}");
        assert!(script.contains("{ 0x7E0094, 2 },"), "{script}");
        assert!(
            script.contains("local pauses = {\n  { emu.memType.snesWorkRam, 0x00019 },\n}"),
            "{script}"
        );
        assert!(
            script.contains(r#"local report = "/tmp/a \"b\".txt""#),
            "{script}"
        );
        // An SA-1 Pack build has the direct page in I-RAM.
        let sa1 = mesen_script(&watches, &breaks, RamMap::Sa1Pack, Path::new("r.txt"));
        assert!(!sa1.contains("0x7E0019"), "{sa1}");
        assert!(sa1.contains("emu.memType.sa1Memory"), "{sa1}");
    }

    #[test]
    fn a_report_reads_back() {
        let report = read_report("frame 120\n1A0 2\npaused 7E0019 03 118\n").unwrap();
        assert_eq!(report.frame, 120);
        assert_eq!(report.values, [0x1A0, 2]);
        assert_eq!(
            report.paused,
            Some(Paused {
                addr: 0x7E_0019,
                value: 3,
                frame: 118
            })
        );
        assert_eq!(read_report("frame 1\n").map(|r| r.frame), None);
        let stopped = read_report("frame 9\n1\nstopped 2 8\n").unwrap();
        assert_eq!((stopped.paused, stopped.stopped), (None, Some((2, 8))));
        assert_eq!(
            read_report("frame 3\n\n").unwrap().values,
            Vec::<u16>::new()
        );
    }
}
