#[path = "fixtures/cpu_hot_path.rs"]
mod probes;

#[test]
fn retirement_preserves_csr_writes_wraparound_traps_and_run_boundaries() {
    probes::retirement();
}

#[test]
fn retirement_counts_completed_host_handoffs() {
    probes::retirement_handoff();
}

#[test]
fn high_multiply_matches_signed_and_unsigned_wide_products() {
    probes::high_multiply();
}
