#[path = "fixtures/pmp.rs"]
mod probes;

#[test]
fn pmp_preserves_priority_partial_matches_and_privilege_permissions() {
    probes::priority_and_permissions();
}

#[test]
fn pmp_handles_empty_tor_ranges_and_locked_predecessors() {
    probes::tor_and_locks();
}

#[test]
fn pmp_csr_changes_invalidate_cached_permissions() {
    probes::invalidation();
}
