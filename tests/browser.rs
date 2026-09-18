use riscbox::browser::{
    BrowserController, BrowserEvent, KeyEvent, PointerEvent, RunPolicy, TerminalSize,
};

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
    controller.network_packet(&[4, 5]);
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
fn run_policy_matches_the_existing_browser_yield_boundary() {
    let policy = RunPolicy::default();
    assert_eq!(policy.blocks_per_slice(), 15);
    assert_eq!(policy.scheduled_delay(0), 0);
    assert_eq!(policy.scheduled_delay(7), 7);
    assert_eq!(policy.scheduled_delay(100), 10);
}
