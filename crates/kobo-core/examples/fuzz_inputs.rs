//! Repeatable parser mutation fuzzing without ROMs or extra dependencies.
//! `cargo run --release --example fuzz_inputs -- [count=10000] [seed=1]`
//! A failing seed is sufficient to reproduce the exact generated input.

#[path = "../tests/common/fuzz.rs"]
mod fuzz;

fn main() {
    let mut args = std::env::args().skip(1);
    let count: u64 = args
        .next()
        .map(|s| s.parse().expect("count must be an integer"))
        .unwrap_or(10_000);
    let first: u64 = args
        .next()
        .map(|s| s.parse().expect("seed must be an integer"))
        .unwrap_or(1);
    for n in 0..count {
        let seed = first.wrapping_add(n);
        if std::panic::catch_unwind(|| fuzz::case(seed)).is_err() {
            eprintln!("reproduce: cargo run --release --example fuzz_inputs -- 1 {seed}");
            std::process::exit(1);
        }
    }
    println!("Checked {count} mutation cases starting at seed {first}");
}
