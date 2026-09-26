use riscbox::browser::{
    BrowserController, BrowserEvent, KeyEvent, NetworkInputResult, PointerEvent, RunPolicy,
    TerminalSize,
};
use riscbox::virtio_devices::MAX_PENDING_NETWORK_FRAMES;

#[test]
fn console_fifo_wraps_and_drops_only_excess_input() {
    let mut controller = BrowserController::default();
    assert_eq!(controller.queue_console(&vec![1; 1000]), 1000);
    let mut first = [0; 900];
    assert_eq!(controller.read_console(&mut first), 900);
    assert_eq!(controller.queue_console(&vec![2; 1000]), 924);
    let mut remaining = vec![0; 1024];
    assert_eq!(controller.read_console(&mut remaining), 1024);
    assert_eq!(&remaining[..100], &[1; 100]);
    assert!(remaining[100..].iter().all(|byte| *byte == 2));
}

#[test]
fn latest_resize_is_consumed_once() {
    let mut controller = BrowserController::default();
    controller.resize(80, 25);
    controller.resize(120, 40);
    assert_eq!(
        controller.take_resize(),
        Some(TerminalSize {
            columns: 120,
            rows: 40
        })
    );
    assert_eq!(controller.take_resize(), None);
}

#[test]
fn browser_events_retain_order_and_wheel_uses_last_pointer_state() {
    let mut controller = BrowserController::default();
    controller.key_event(true, 30);
    controller.pointer_event(10, 20, 3);
    controller.wheel_event(-1);
    assert_eq!(
        controller.network_packet(&[4, 5]),
        NetworkInputResult::Accepted
    );
    controller.network_carrier(true);
    assert_eq!(
        controller.next_event(),
        Some(BrowserEvent::Key(KeyEvent {
            pressed: true,
            code: 30
        }))
    );
    assert_eq!(
        controller.next_event(),
        Some(BrowserEvent::Pointer(PointerEvent {
            x: 10,
            y: 20,
            wheel: 0,
            buttons: 3
        }))
    );
    assert_eq!(
        controller.next_event(),
        Some(BrowserEvent::Pointer(PointerEvent {
            x: 10,
            y: 20,
            wheel: -1,
            buttons: 3
        }))
    );
    assert_eq!(
        controller.next_event(),
        Some(BrowserEvent::NetworkPacket(vec![4, 5]))
    );
    assert_eq!(
        controller.next_event(),
        Some(BrowserEvent::NetworkCarrier(true))
    );
    assert!(controller.carrier_is_up());
}

#[test]
fn browser_network_ingress_is_bounded_and_recovers_after_drain() {
    let mut controller = BrowserController::default();
    assert_eq!(controller.network_packet(&[]), NetworkInputResult::Dropped);
    for _ in 0..MAX_PENDING_NETWORK_FRAMES {
        assert_eq!(
            controller.network_packet(&[1]),
            NetworkInputResult::Accepted
        );
    }
    assert_eq!(controller.network_packet(&[2]), NetworkInputResult::Dropped);
    assert!(matches!(
        controller.next_event(),
        Some(BrowserEvent::NetworkPacket(_))
    ));
    assert_eq!(
        controller.network_packet(&[3]),
        NetworkInputResult::Accepted
    );
}

#[test]
fn run_policy_allows_the_driver_to_choose_the_guest_cycle_budget() {
    let policy = RunPolicy::default();
    assert_eq!(policy.yield_cycles, i32::MAX as u32);
    assert_eq!(policy.maximum_delay_ms, 100);
}
