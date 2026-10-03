#[path = "common/fuzz.rs"]
mod fuzz;

#[test]
fn mutated_headers_pointers_streams_and_truncations_do_not_panic() {
    for seed in 1..=512 {
        let result = std::panic::catch_unwind(|| fuzz::case(seed));
        assert!(
            result.is_ok(),
            "reproduce with: cargo run --example fuzz_inputs -- 1 {seed}"
        );
    }
}
