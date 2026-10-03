//! Runs the 65816 core against the SingleStepTests suite
//! (<https://github.com/SingleStepTests/65816>): 10,000 native-mode tests
//! per opcode, each one instruction with the full processor and memory
//! state before and after. The ROM-backed tests only exercise the paths
//! SMW takes; custom sprites and patches take others.
//!
//! Set `KOBO_65816_TESTS` to the suite's `v1` directory; only the
//! `*.n.json` files are read. The suite is about 1.7 GiB, carries no
//! licence, and is never committed. Without the variable the test prints
//! `skipping` and passes.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use kobo_core::cpu::{Bus, Cpu};
use serde::Deserialize;

/// Opcodes the core refuses by design: it models no interrupts, so
/// `BRK`, `COP`, and `STP` stop a run with an error, and `WAI` waits for an
/// interrupt the suite does not model.
const UNMODELLED: [u8; 4] = [0x00, 0x02, 0xCB, 0xDB];
/// `MVP` and `MVN`. The suite cuts every test off after 100 bus cycles,
/// mid-move, while the core runs a block move to completion in one step.
const BLOCK_MOVES: [u8; 2] = [0x44, 0x54];

#[derive(Deserialize)]
struct State {
    pc: u16,
    s: u16,
    p: u8,
    a: u16,
    x: u16,
    y: u16,
    dbr: u8,
    d: u16,
    pbr: u8,
    ram: Vec<(u32, u8)>,
}

#[derive(Deserialize)]
struct Test {
    name: String,
    initial: State,
    #[serde(rename = "final")]
    expected: State,
}

/// 16 MiB of memory holding only the bytes a test names.
struct SparseRam(HashMap<u32, u8>);

impl Bus for SparseRam {
    fn read(&mut self, addr: u32) -> u8 {
        self.0.get(&addr).copied().unwrap_or(0)
    }

    fn write(&mut self, addr: u32, value: u8) {
        self.0.insert(addr, value);
    }
}

/// Runs one test; `Err` describes the first difference.
fn run(test: &Test, opcode: u8) -> Result<(), String> {
    let (initial, expected) = (&test.initial, &test.expected);
    let mut cpu = Cpu::new();
    cpu.pc = initial.pc;
    cpu.sp = initial.s;
    cpu.p = initial.p;
    cpu.a = initial.a;
    cpu.x = initial.x;
    cpu.y = initial.y;
    cpu.db = initial.dbr;
    cpu.dp = initial.d;
    cpu.pb = initial.pbr;
    // A block move the suite cut short moved `a - expected.a` bytes. Give
    // the core exactly that many to move, and leave out what only a
    // finished move settles: the accumulator and the program counter.
    let cut_short = BLOCK_MOVES.contains(&opcode) && expected.a != 0xFFFF;
    if cut_short {
        cpu.a = initial.a.wrapping_sub(expected.a).wrapping_sub(1);
    }
    let mut ram = SparseRam(initial.ram.iter().copied().collect());
    cpu.step(&mut ram).map_err(|e| e.to_string())?;

    let mut registers = vec![
        ("s", cpu.sp as u32, expected.s as u32),
        ("p", cpu.p as u32, expected.p as u32),
        ("x", cpu.x as u32, expected.x as u32),
        ("y", cpu.y as u32, expected.y as u32),
        ("dbr", cpu.db as u32, expected.dbr as u32),
        ("d", cpu.dp as u32, expected.d as u32),
        ("pbr", cpu.pb as u32, expected.pbr as u32),
    ];
    if !cut_short {
        registers.push(("pc", cpu.pc as u32, expected.pc as u32));
        registers.push(("a", cpu.a as u32, expected.a as u32));
    }
    for (name, got, want) in registers {
        if got != want {
            return Err(format!("{name} = {got:#X}, expected {want:#X}"));
        }
    }
    let want: HashMap<u32, u8> = expected.ram.iter().copied().collect();
    for (&addr, &value) in &want {
        let got = ram.0.get(&addr).copied().unwrap_or(0);
        if got != value {
            return Err(format!("${addr:06X} = {got:#04X}, expected {value:#04X}"));
        }
    }
    if let Some(addr) = ram.0.keys().find(|addr| !want.contains_key(addr)) {
        return Err(format!("stray write to ${addr:06X}"));
    }
    Ok(())
}

/// Failures in one opcode's file: how many, and the first.
fn run_file(path: &Path, opcode: u8) -> (usize, Option<String>) {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let tests: Vec<Test> =
        serde_json::from_slice(&data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(
        !tests.is_empty(),
        "{}: CPU fixture contains no cases",
        path.display()
    );
    let mut failures = 0;
    let mut first = None;
    for test in &tests {
        if let Err(why) = run(test, opcode) {
            failures += 1;
            first.get_or_insert_with(|| format!("{}: {why}", test.name));
        }
    }
    (failures, first)
}

#[test]
fn core_matches_the_single_step_suite() {
    let Some(dir) = std::env::var_os("KOBO_65816_TESTS").map(PathBuf::from) else {
        eprintln!("skipping: KOBO_65816_TESTS is not set");
        return;
    };
    let opcodes: Vec<u8> = (0..=255u8).filter(|op| !UNMODELLED.contains(op)).collect();
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let results: BTreeMap<u8, (usize, Option<String>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let (dir, opcodes) = (&dir, &opcodes);
                scope.spawn(move || {
                    opcodes
                        .iter()
                        .skip(worker)
                        .step_by(workers)
                        .map(|&op| (op, run_file(&dir.join(format!("{op:02x}.n.json")), op)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect()
    });
    let failed: Vec<String> = results
        .iter()
        .filter(|(_, (failures, _))| *failures > 0)
        .map(|(op, (failures, first))| {
            format!(
                "opcode {op:02X}: {failures} failures, first {}",
                first.as_deref().unwrap_or("?")
            )
        })
        .collect();
    eprintln!("checked {} opcodes", results.len());
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
