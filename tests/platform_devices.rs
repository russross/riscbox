use riscbox::cpu::{MIP_MEIP, MIP_SEIP};
use riscbox::platform::{Clint, FinishStatus, Finisher, GoldfishRtc, Plic, Uart16550};

#[test]
fn clint_exposes_split_time_compare_and_software_interrupt() {
    let mut clint = Clint::default();
    assert_eq!(clint.read(0xbff8, 0x1234_5678_9abc_def0), 0x9abc_def0);
    assert_eq!(clint.read(0xbffc, 0x1234_5678_9abc_def0), 0x1234_5678);
    clint.write(0x4000, 0x7654_3210);
    clint.write(0x4004, 0x0000_0001);
    assert_eq!(clint.timecmp(), 0x0000_0001_7654_3210);
    assert!(!clint.timer_interrupt(clint.timecmp() - 1));
    assert!(clint.timer_interrupt(clint.timecmp()));
    clint.write(0, 3);
    assert!(clint.software_interrupt());
    clint.write(0, 2);
    assert!(!clint.software_interrupt());
}

#[test]
fn plic_honors_priority_threshold_claim_and_level_repending() {
    let mut plic = Plic::default();
    plic.write(4 * 5, 3);
    plic.write(4 * 7, 3);
    plic.write(0x2000, (1 << 5) | (1 << 7));
    plic.set_irq(7, true);
    plic.set_irq(5, true);
    assert_eq!(plic.cpu_interrupts(), MIP_MEIP);
    assert_eq!(plic.read(0x20_0004), 5);
    assert_eq!(plic.read(0x20_0004), 7);
    plic.write(0x20_0004, 5);
    assert_eq!(plic.read(0x20_0004), 5);
    plic.set_irq(5, false);
    plic.write(0x20_0004, 5);
    plic.set_irq(7, false);
    plic.write(0x20_0004, 7);
    assert_eq!(plic.cpu_interrupts(), 0);

    plic.set_irq(7, true);
    plic.write(0x20_0000, 3);
    assert_eq!(plic.cpu_interrupts(), 0);
    assert_eq!(plic.read(0x20_0004), 0);
    plic.write(0x2080, 1 << 7);
    plic.write(0x20_1000, 2);
    assert_eq!(plic.cpu_interrupts(), MIP_SEIP);
}

#[test]
fn uart_models_fifo_interrupts_loopback_and_transmit() {
    let mut uart = Uart16550::default();
    uart.write(3, 0x80);
    uart.write(0, 12);
    uart.write(1, 1);
    assert_eq!(uart.read(0), 12);
    assert_eq!(uart.read(1), 1);
    uart.write(3, 3);
    uart.write(2, 0x41);
    uart.write(1, 1);
    assert_eq!(uart.receive(b"abc"), 3);
    assert_eq!(uart.read(2), 0x0c | 0xc0);
    assert_eq!(uart.receive_space(), 13);
    assert_eq!(uart.read(0), b'a');
    uart.write(4, 0x10);
    assert_eq!(uart.receive_space(), 0);
    uart.write(0, b'z');
    assert_eq!(uart.read(0), b'b');
    assert!(uart.take_transmitted().is_empty());
    uart.write(4, 0);
    uart.write(0, b'!');
    assert_eq!(uart.take_transmitted(), b"!");
}

#[test]
fn goldfish_rtc_latches_time_and_alarm_interrupts() {
    let mut rtc = GoldfishRtc::default();
    let now = 0x1234_5678_9abc_def0;
    assert_eq!(rtc.read(0, now), 0x9abc_def0);
    assert_eq!(rtc.read(4, 0), 0x1234_5678);
    let alarm = now + 2_000_000;
    rtc.write(0x0c, (alarm >> 32) as u32, now);
    let alarm_bytes = alarm.to_le_bytes();
    rtc.write(
        8,
        u32::from_le_bytes([
            alarm_bytes[0],
            alarm_bytes[1],
            alarm_bytes[2],
            alarm_bytes[3],
        ]),
        now,
    );
    rtc.write(0x10, 1, now);
    assert!(!rtc.irq());
    assert_eq!(rtc.limit_delay_ms(10, now), 2);
    assert_eq!(rtc.limit_delay_ms(10, alarm), 10);
    assert!(rtc.irq());
    rtc.write(0x1c, 0, now);
    assert!(!rtc.irq());
}

#[test]
fn finisher_records_pass_and_failure_without_exiting() {
    let mut finisher = Finisher::default();
    finisher.write(4, 0x5555);
    assert_eq!(finisher.status(), FinishStatus::Running);
    finisher.write(0, 0x5555);
    assert_eq!(finisher.status(), FinishStatus::Passed);
    finisher.write(0, 0x002a_3333);
    assert_eq!(finisher.status(), FinishStatus::Failed(42));
}
