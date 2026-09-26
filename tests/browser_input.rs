use riscbox::browser_input::{
    BrowserEvent, BrowserInputQueue, KeyEvent, NetworkInputResult, PointerEvent, TerminalSize,
};
use riscbox::virtio_devices::MAX_PENDING_NETWORK_FRAMES;

#[test]
fn console_fifo_wraps_and_drops_only_excess_input() {
    let mut input_queue = BrowserInputQueue::default();
    assert_eq!(input_queue.queue_console(&vec![1; 1000]), 1000);
    let mut first = [0; 900];
    assert_eq!(input_queue.read_console(&mut first), 900);
    assert_eq!(input_queue.queue_console(&vec![2; 1000]), 924);
    let mut remaining = vec![0; 1024];
    assert_eq!(input_queue.read_console(&mut remaining), 1024);
    assert_eq!(&remaining[..100], &[1; 100]);
    assert!(remaining[100..].iter().all(|byte| *byte == 2));
}

#[test]
fn latest_resize_is_consumed_once() {
    let mut input_queue = BrowserInputQueue::default();
    input_queue.resize(80, 25);
    input_queue.resize(120, 40);
    assert_eq!(
        input_queue.take_resize(),
        Some(TerminalSize {
            columns: 120,
            rows: 40
        })
    );
    assert_eq!(input_queue.take_resize(), None);
}

#[test]
fn browser_events_retain_order_and_wheel_uses_last_pointer_state() {
    let mut input_queue = BrowserInputQueue::default();
    input_queue.key_event(true, 30);
    input_queue.pointer_event(10, 20, 3);
    input_queue.wheel_event(-1);
    assert_eq!(
        input_queue.network_packet(&[4, 5]),
        NetworkInputResult::Accepted
    );
    input_queue.network_carrier(true);
    assert_eq!(
        input_queue.next_event(),
        Some(BrowserEvent::Key(KeyEvent {
            pressed: true,
            code: 30
        }))
    );
    assert_eq!(
        input_queue.next_event(),
        Some(BrowserEvent::Pointer(PointerEvent {
            x: 10,
            y: 20,
            wheel: 0,
            buttons: 3
        }))
    );
    assert_eq!(
        input_queue.next_event(),
        Some(BrowserEvent::Pointer(PointerEvent {
            x: 10,
            y: 20,
            wheel: -1,
            buttons: 3
        }))
    );
    assert_eq!(
        input_queue.next_event(),
        Some(BrowserEvent::NetworkPacket(vec![4, 5]))
    );
    assert_eq!(
        input_queue.next_event(),
        Some(BrowserEvent::NetworkCarrier(true))
    );
    assert!(input_queue.carrier_is_up());
}

#[test]
fn browser_network_ingress_is_bounded_and_recovers_after_drain() {
    let mut input_queue = BrowserInputQueue::default();
    assert_eq!(input_queue.network_packet(&[]), NetworkInputResult::Dropped);
    for _ in 0..MAX_PENDING_NETWORK_FRAMES {
        assert_eq!(
            input_queue.network_packet(&[1]),
            NetworkInputResult::Accepted
        );
    }
    assert_eq!(
        input_queue.network_packet(&[2]),
        NetworkInputResult::Dropped
    );
    assert!(matches!(
        input_queue.next_event(),
        Some(BrowserEvent::NetworkPacket(_))
    ));
    assert_eq!(
        input_queue.network_packet(&[3]),
        NetworkInputResult::Accepted
    );
}
