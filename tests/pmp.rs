#[path = "fixtures/pmp.rs"]
mod probes;

#[test]
fn pmp_does_not_restrict_memory_accesses() {
    probes::memory_accesses();
}

#[test]
fn pmp_csr_handlers_preserve_masks_and_locks() {
    probes::csr_handlers();
}
