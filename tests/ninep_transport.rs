#[path = "fixtures/ninep_transport.rs"]
mod probe;

#[test]
fn rust_ninep_uses_real_guest_rings_and_separate_reset_lifetimes() {
    probe::regression();
}
