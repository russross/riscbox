use riscbox::browser_input::BrowserInputQueue;
use riscbox::browser_runtime::{
    BrowserNineP, BrowserRuntime, HostAction, HostBlockAction, LifecycleCause, QuantumOutcome,
    QuantumStart, RuntimeError, RuntimeStart,
};
use riscbox::config::VmConfig;
use riscbox::host_block::HostBlockProviderId;
use riscbox::ninep::{FileRead, Filesystem, Limits};
use riscbox::ninep_backend::RustFilesystem;
use riscbox::virtio_devices::{
    NinePBackend, NinePEndpointId, NinePGeneration, NinePRequestId, NinePTransportAction,
};

fn start() -> RuntimeStart {
    RuntimeStart {
        config_url: "https://host/vm/riscbox.cfg".into(),
        ram_mib: 32,
        command_line: "quiet".into(),
        width: 0,
        height: 0,
        has_network: false,
    }
}

#[test]
fn filesystem_handle_claims_one_vm_and_releases_on_destroy_or_failed_start() {
    let tree = RustFilesystem::new(Filesystem::new(Limits::default(), 100));
    let config = VmConfig::from_resolved(
        r#"{"version":1,"machine":"riscv64","memory_size":32,"console":"uart",
        "uart_output":true,"rtc_local_time":false,"cmdline":"","bios":"https://host/fw.bin",
        "fs0":{"server":"share","tag":"first"},"fs1":{"server":"alias","tag":"second"}}"#,
    )
    .unwrap();
    let mut first = BrowserRuntime::default();
    first
        .register_filesystem_handle("share".into(), tree.clone())
        .unwrap();
    first
        .register_filesystem_handle("alias".into(), tree.clone())
        .unwrap();
    first.start_resolved(start(), config.clone()).unwrap();
    let (id, _) = request(&mut first);
    first
        .complete_http(id, 200, vec![0x73, 0, 0x50, 0x10])
        .unwrap();
    assert!(tree.is_attached());
    let mut second = BrowserRuntime::default();
    second
        .register_filesystem_handle("share".into(), tree.clone())
        .unwrap();
    second
        .register_filesystem_handle("alias".into(), tree.clone())
        .unwrap();
    second.start_resolved(start(), config.clone()).unwrap();
    let (id, _) = request(&mut second);
    assert!(
        second
            .complete_http(id, 200, vec![0x73, 0, 0x50, 0x10])
            .is_err()
    );
    first.reset().unwrap();
    assert!(tree.is_attached());
    first.halt().unwrap();
    assert!(tree.is_attached());
    first.destroy().unwrap();
    assert!(!tree.is_attached());
    second.start_resolved(start(), config.clone()).unwrap();
    let (id, _) = request(&mut second);
    assert!(second.complete_http(id, 200, Vec::new()).is_err());
    assert!(!tree.is_attached());
    second.start_resolved(start(), config).unwrap();
    let (id, _) = request(&mut second);
    second
        .complete_http(id, 200, vec![0x73, 0, 0x50, 0x10])
        .unwrap();
    assert!(tree.is_attached());
    second.halt().unwrap();
    second.destroy().unwrap();
    assert!(!tree.is_attached());
}

#[test]
fn registered_rust_namespace_exists_before_boot_and_survives_vm_lifetimes() {
    let mut runtime = BrowserRuntime::default();
    runtime
        .register_filesystem("workspace".into(), Filesystem::new(Limits::default(), 100))
        .unwrap();
    runtime
        .with_filesystem("workspace", |fs| fs.write_file("host", b"before"))
        .unwrap();
    assert!(
        runtime
            .register_filesystem("workspace".into(), Filesystem::new(Limits::default(), 100))
            .is_err()
    );
    for _ in 0..2 {
        let config = VmConfig::from_resolved(
            r#"{"version":1,"machine":"riscv64","memory_size":32,"console":"uart",
            "uart_output":true,"rtc_local_time":false,"cmdline":"",
            "bios":"https://host/fw.bin","fs0":{"server":"workspace","tag":"shared"},
            "fs1":{"server":"workspace","tag":"peer"}}"#,
        )
        .unwrap();
        runtime.start_resolved(start(), config).unwrap();
        let (firmware, _) = request(&mut runtime);
        runtime
            .complete_http(firmware, 200, vec![0x73, 0, 0x50, 0x10])
            .unwrap();
        assert_eq!(runtime.next_action(), Some(HostAction::Started));
        assert!(runtime.next_action().is_none());
        assert_eq!(
            runtime
                .with_filesystem("workspace", |fs| fs.read_file("host"))
                .unwrap(),
            FileRead::Resident(b"before".to_vec())
        );
        runtime.reset().unwrap();
        assert_eq!(
            runtime
                .with_filesystem("workspace", |fs| fs.read_file("host"))
                .unwrap(),
            FileRead::Resident(b"before".to_vec())
        );
        runtime.halt().unwrap();
        runtime.destroy().unwrap();
        assert!(runtime.next_ninep_load().is_none());
    }
    runtime
        .with_filesystem("workspace", Filesystem::reset)
        .unwrap();
    assert!(
        runtime
            .with_filesystem("workspace", |fs| fs.read_file("host"))
            .is_err()
    );
}

#[test]
fn resolved_start_requests_assets_without_fetching_configuration() {
    let config = VmConfig::from_resolved(
        r#"{"version":1,"machine":"riscv64","memory_size":32,
        "console":"uart","uart_output":false,"rtc_local_time":false,
        "cmdline":"","bios":"https://host/firmware.bin",
        "drive0":{"file":"https://host/disk/blk.txt"}}"#,
    )
    .expect("resolved configuration");
    let mut runtime = BrowserRuntime::default();
    runtime.start_resolved(start(), config).expect("start");
    assert_eq!(request(&mut runtime).1, "https://host/firmware.bin");
}

#[test]
fn resolved_mixed_drives_load_in_guest_order() {
    let config = VmConfig::from_resolved(
        r#"{"version":1,"machine":"riscv64","memory_size":32,
        "console":"uart","uart_output":false,"rtc_local_time":false,"cmdline":"",
        "bios":"https://host/firmware.bin",
        "drive0":{"provider":7,"capacity_sectors":"8"},
        "drive1":{"file":"https://host/disk/blk.txt"}}"#,
    )
    .expect("mixed drives");
    let mut runtime = BrowserRuntime::default();
    runtime.start_resolved(start(), config).expect("start");
    let (firmware_id, firmware_url) = request(&mut runtime);
    assert_eq!(firmware_url, "https://host/firmware.bin");
    runtime
        .complete_http(firmware_id, 200, vec![0x13, 0, 0, 0])
        .expect("firmware");
    let (manifest_id, manifest_url) = request(&mut runtime);
    assert_eq!(manifest_url, "https://host/disk/blk.txt");
    runtime
        .complete_http(manifest_id, 200, b"{block_size:1,n_block:1}".to_vec())
        .expect("manifest");
    assert_eq!(runtime.next_action(), Some(HostAction::Started));
    runtime.halt().expect("halt");
    runtime.destroy().expect("destroy");
    assert!(matches!(
        runtime.next_action(),
        Some(HostAction::HostBlock(HostBlockAction::Close {
            provider: HostBlockProviderId(7)
        }))
    ));
}

#[test]
fn host_halt_boot_reset_and_destroy_preserve_the_vm_until_destroyed() {
    let mut runtime = start_uart_writer(
        br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"uart"}"#,
    );
    runtime.request_shutdown().expect("deliver power key");
    runtime.request_reboot().expect("deliver restart key");
    assert!(runtime.is_running());
    runtime.halt().expect("forced halt");
    assert!(runtime.request_shutdown().is_err());
    assert!(runtime.is_halted());
    assert_eq!(
        runtime.next_action(),
        Some(HostAction::Halted(LifecycleCause::HostHalt))
    );
    assert_eq!(runtime.begin_quantum(1_000_000), QuantumStart::VmInactive);
    runtime.reset().expect("boot retained machine");
    assert!(runtime.is_running());
    assert_eq!(
        runtime.next_action(),
        Some(HostAction::Reset(LifecycleCause::HostBoot))
    );
    runtime.reset().expect("forced reset");
    assert_eq!(
        runtime.next_action(),
        Some(HostAction::Reset(LifecycleCause::HostReset))
    );
    runtime.halt().expect("halt before destroy");
    assert_eq!(
        runtime.next_action(),
        Some(HostAction::Halted(LifecycleCause::HostHalt))
    );
    runtime.destroy().expect("destroy halted VM");
    assert!(!runtime.is_halted());
    assert!(runtime.reset().is_err());
}

#[test]
fn guest_finisher_reset_reboots_in_place_and_poweroff_halts() {
    for (value, expected) in [
        (0x7777_u32, HostAction::Reset(LifecycleCause::GuestReboot)),
        (
            0x5555_u32,
            HostAction::Halted(LifecycleCause::GuestPoweroff),
        ),
    ] {
        let mut runtime = BrowserRuntime::default();
        runtime.start(start()).expect("start");
        let (config_id, _) = request(&mut runtime);
        runtime
            .complete_http(
                config_id,
                200,
                br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"uart"}"#
                    .to_vec(),
            )
            .expect("config");
        let (firmware_id, _) = request(&mut runtime);
        let upper = value >> 12;
        let lower = value & 0xfff;
        let instructions = [
            0x0010_00b7_u32,
            (upper << 12) | 0x137,
            (lower << 20) | (2 << 15) | (2 << 7) | 0x13,
            0x0020_a023,
            0x0000_006f,
        ];
        let firmware = instructions
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        runtime
            .complete_http(firmware_id, 200, firmware)
            .expect("firmware");
        assert_eq!(runtime.next_action(), Some(HostAction::Started));
        let mut input = BrowserInputQueue::default();
        runtime
            .run_cpu_once(&mut input, 0, 0, 100)
            .expect("guest run");
        assert_eq!(runtime.next_action(), Some(expected));
        if value == 0x7777 {
            assert!(runtime.is_running());
        } else {
            assert!(runtime.is_halted());
        }
    }
}

fn request(runtime: &mut BrowserRuntime) -> (u32, String) {
    let Some(HostAction::Request(request)) = runtime.next_action() else {
        panic!("expected HTTP request");
    };
    (request.id, request.url)
}

fn start_uart_writer(config: &[u8]) -> BrowserRuntime {
    let mut runtime = BrowserRuntime::default();
    runtime.start(start()).expect("start");
    let (config_id, _) = request(&mut runtime);
    runtime
        .complete_http(config_id, 200, config.to_vec())
        .expect("configuration");
    let (firmware_id, _) = request(&mut runtime);
    let instructions = [0x1000_00b7_u32, 0x0410_0113, 0x0020_8023, 0x0000_006f];
    let firmware = instructions
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    runtime
        .complete_http(firmware_id, 200, firmware)
        .expect("firmware");
    assert_eq!(runtime.next_action(), Some(HostAction::Started));
    assert_eq!(runtime.next_action(), None);
    runtime
}

#[test]
fn complete_quantum_resumes_after_host_action_and_updates_rate() {
    let mut runtime = start_uart_writer(
        br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"uart"}"#,
    );
    runtime
        .configure_quantum(1.0, true)
        .expect("timing configuration");
    assert_eq!(runtime.begin_quantum(1_000_000), QuantumStart::Ready);
    assert_eq!(
        runtime.begin_quantum(1_000_000),
        QuantumStart::AlreadyActive
    );
    let mut input_queue = BrowserInputQueue::default();
    assert_eq!(
        runtime
            .run_quantum(&mut input_queue)
            .expect("first advance"),
        QuantumOutcome::HostServiceRequired
    );
    assert!(matches!(
        runtime.next_action(),
        Some(HostAction::Console(_))
    ));
    assert_eq!(runtime.next_action(), None);
    assert_eq!(
        runtime
            .run_quantum(&mut input_queue)
            .expect("remaining budget"),
        QuantumOutcome::BudgetReached
    );
    assert_eq!(runtime.finish_quantum(2.0, 1_000_002).expect("finish"), 0);
    assert!(runtime.timing_stat(0) > 0.0);
    assert!(runtime.timing_stat(1) >= 1.0);
    assert_eq!(runtime.begin_quantum(1_000_000), QuantumStart::Ready);
    runtime.abort_quantum();
}

#[test]
fn clock_skew_and_carried_budget_share_ahead_of_wall_time() {
    let mut runtime = start_uart_writer(
        br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"uart"}"#,
    );
    runtime
        .configure_quantum(20.0, true)
        .expect("timing configuration");
    let mut input_queue = BrowserInputQueue::default();

    // A fast quantum advances less than twenty milliseconds of guest time
    // under the initial twenty percent skew.
    assert_eq!(runtime.begin_quantum(1_000_000), QuantumStart::Ready);
    assert_eq!(
        runtime
            .run_quantum(&mut input_queue)
            .expect("console output"),
        QuantumOutcome::HostServiceRequired
    );
    assert!(matches!(
        runtime.next_action(),
        Some(HostAction::Console(_))
    ));
    assert_eq!(
        runtime.run_quantum(&mut input_queue).expect("first budget"),
        QuantumOutcome::BudgetReached
    );
    runtime
        .finish_quantum(15.0, 1_000_015)
        .expect("first finish");
    let rate = runtime.timing_stat(0);

    // The carried lead extends the next budget while keeping the skewed clock.
    assert_eq!(runtime.begin_quantum(1_000_015), QuantumStart::Ready);
    assert_eq!(
        runtime
            .run_quantum(&mut input_queue)
            .expect("second budget"),
        QuantumOutcome::BudgetReached
    );
    runtime
        .finish_quantum(25.0, 1_000_040)
        .expect("second finish");
    assert!(runtime.timing_stat(6) > 0.0);
    assert!(runtime.timing_stat(7) > 0.0);
    let second_cycles = runtime.timing_stat(4);
    assert!(second_cycles > rate * 0.020);

    // Once host time passes the presented guest clock, the nominal budget applies.
    assert_eq!(runtime.begin_quantum(1_000_050), QuantumStart::Ready);
    assert_eq!(
        runtime
            .run_quantum(&mut input_queue)
            .expect("nominal budget"),
        QuantumOutcome::BudgetReached
    );
    runtime
        .finish_quantum(20.0, 1_000_070)
        .expect("third finish");
    assert!(runtime.timing_stat(4) > 0.0);
}

#[test]
fn configuration_and_boot_assets_load_in_dependency_order() {
    let mut runtime = BrowserRuntime::default();
    runtime.start(start()).expect("start");
    let (config_id, url) = request(&mut runtime);
    assert_eq!(url, "https://host/vm/riscbox.cfg");

    runtime
        .complete_http(
            config_id,
            200,
            br#"{version:1,machine:"riscv64",memory_size:128,bios:"fw.bin",kernel:"linux",initrd:"initrd.img",console:"uart"}"#.to_vec(),
        )
        .expect("configuration");
    let (firmware_id, url) = request(&mut runtime);
    assert_eq!(url, "https://host/vm/fw.bin");
    runtime
        .complete_http(firmware_id, 200, vec![0; 64])
        .expect("firmware");
    let (kernel_id, url) = request(&mut runtime);
    assert_eq!(url, "https://host/vm/linux");
    runtime
        .complete_http(kernel_id, 200, vec![0; 64])
        .expect("kernel");
    let (initrd_id, url) = request(&mut runtime);
    assert_eq!(url, "https://host/vm/initrd.img");
    runtime
        .complete_http(initrd_id, 200, vec![0; 64])
        .expect("initrd");

    assert!(runtime.is_running());
    assert_eq!(runtime.next_action(), Some(HostAction::Started));
    assert_eq!(runtime.next_action(), None);
}

#[test]
fn browser_boots_without_firmware_and_rejects_an_empty_firmware_file() {
    let mut runtime = BrowserRuntime::default();
    runtime.start(start()).expect("start");
    let (config_id, _) = request(&mut runtime);
    runtime.complete_http(config_id, 200,
        br#"{version:1,machine:"riscv64",memory_size:32,kernel:"bare.bin",kernel_address:"0x80400000"}"#.to_vec()
    ).expect("configuration");
    let (kernel_id, url) = request(&mut runtime);
    assert_eq!(url, "https://host/vm/bare.bin");
    runtime
        .complete_http(kernel_id, 200, vec![0x13; 64])
        .expect("direct kernel");
    assert_eq!(runtime.next_action(), Some(HostAction::Started));

    let mut invalid = BrowserRuntime::default();
    invalid.start(start()).expect("start");
    let (config_id, _) = request(&mut invalid);
    invalid
        .complete_http(
            config_id,
            200,
            br#"{version:1,machine:"riscv64",memory_size:32,bios:"empty.bin"}"#.to_vec(),
        )
        .expect("configuration");
    let (firmware_id, _) = request(&mut invalid);
    let error = invalid
        .complete_http(firmware_id, 200, vec![])
        .expect_err("empty firmware");
    assert!(error.to_string().contains("firmware is empty"));
}

#[test]
fn responses_must_match_the_single_pending_request() {
    let mut runtime = BrowserRuntime::default();
    runtime.start(start()).expect("start");
    let (config_id, _) = request(&mut runtime);
    assert_eq!(
        runtime.complete_http(config_id + 1, 200, Vec::new()),
        Err(RuntimeError::UnexpectedResponse(config_id + 1))
    );
    assert_eq!(
        runtime.complete_http(config_id, 404, Vec::new()),
        Err(RuntimeError::HttpStatus(404))
    );
}

#[test]
fn run_delivers_queued_input_and_reschedules_runnable_guest_immediately() {
    let mut runtime = BrowserRuntime::default();
    runtime.start(start()).expect("start");
    let (config_id, _) = request(&mut runtime);
    runtime
        .complete_http(
            config_id,
            200,
            br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"uart"}"#
                .to_vec(),
        )
        .expect("configuration");
    let (firmware_id, _) = request(&mut runtime);
    runtime
        .complete_http(firmware_id, 200, vec![0; 64])
        .expect("firmware");
    assert_eq!(runtime.next_action(), Some(HostAction::Started));
    assert_eq!(runtime.next_action(), None);

    let mut input_queue = BrowserInputQueue::default();
    assert_eq!(input_queue.queue_console(b"x"), 1);
    runtime
        .run_cpu_once(&mut input_queue, 0, 0, 3_000_000)
        .expect("execution slice");
    assert_eq!(runtime.next_action(), None);
}

#[test]
fn uart_backpressure_retains_unaccepted_browser_input() {
    let mut runtime = BrowserRuntime::default();
    runtime.start(start()).expect("start");
    let (config_id, _) = request(&mut runtime);
    runtime
        .complete_http(
            config_id,
            200,
            br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"uart"}"#
                .to_vec(),
        )
        .expect("configuration");
    let (firmware_id, _) = request(&mut runtime);
    runtime
        .complete_http(firmware_id, 200, vec![0; 64])
        .expect("firmware");
    runtime.next_action();
    runtime.next_action();

    let mut input_queue = BrowserInputQueue::default();
    input_queue.queue_console(b"ABC");
    runtime
        .run_cpu_once(&mut input_queue, 0, 0, 3_000_000)
        .expect("execution slice");
    assert_eq!(input_queue.console_len(), 2);
}

#[test]
fn virtio_input_before_driver_initialization_is_buffered() {
    let mut runtime = BrowserRuntime::default();
    runtime.start(start()).expect("start");
    let (config_id, _) = request(&mut runtime);
    runtime
        .complete_http(
            config_id,
            200,
            br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"virtio",input_device:"virtio",eth0:{driver:"user"}}"#.to_vec(),
        )
        .expect("configuration");
    let (firmware_id, _) = request(&mut runtime);
    runtime
        .complete_http(firmware_id, 200, vec![0; 64])
        .expect("firmware");
    runtime.next_action();
    runtime.next_action();

    let mut input_queue = BrowserInputQueue::default();
    input_queue.queue_console(b"x");
    input_queue.key_event(true, 30);
    input_queue.pointer_event(50, 60, 0);
    assert_eq!(
        input_queue.network_packet(b"frame"),
        riscbox::browser_input::NetworkInputResult::Accepted
    );
    runtime
        .run_cpu_once(&mut input_queue, 0, 0, 3_000_000)
        .expect("execution slice");
    assert_eq!(runtime.next_action(), None);
}

#[test]
fn uart_output_follows_the_console_configuration() {
    let cases: [(&[u8], bool); 3] = [
        (
            br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"virtio"}"#,
            false,
        ),
        (
            br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"virtio",uart_output:true}"#,
            true,
        ),
        (
            br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"uart"}"#,
            true,
        ),
    ];

    for (config, expects_output) in cases {
        let mut runtime = start_uart_writer(config);
        runtime
            .run_cpu_once(&mut BrowserInputQueue::default(), 0, 0, 3_000_000)
            .expect("execution slice");
        if expects_output {
            assert_eq!(
                runtime.next_action(),
                Some(HostAction::Console(b"A".to_vec()))
            );
        }
        assert_eq!(runtime.next_action(), None);
    }
}

#[test]
fn drive_manifest_precedes_machine_start_and_prefetch_requests_follow_it() {
    let mut runtime = BrowserRuntime::default();
    runtime.start(start()).expect("start");
    let (config_id, _) = request(&mut runtime);
    runtime
        .complete_http(
            config_id,
            200,
            br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",drive0:{file:"disk/blk.txt"},console:"uart"}"#.to_vec(),
        )
        .expect("configuration");
    let (firmware_id, _) = request(&mut runtime);
    runtime
        .complete_http(firmware_id, 200, vec![0; 64])
        .expect("firmware");
    let (manifest_id, url) = request(&mut runtime);
    assert_eq!(url, "https://host/vm/disk/blk.txt");
    runtime
        .complete_http(
            manifest_id,
            200,
            br"{block_size:1,n_block:2,prefetch:[1]}".to_vec(),
        )
        .expect("manifest");
    assert_eq!(runtime.next_action(), Some(HostAction::Started));
    let (old_request, url) = request(&mut runtime);
    assert_eq!(url, "https://host/vm/disk/blk000000001.bin");
    runtime.reset().expect("reset with pending prefetch");
    assert_eq!(
        runtime.next_action(),
        Some(HostAction::Reset(LifecycleCause::HostReset))
    );
    assert_eq!(
        runtime.complete_http(old_request, 200, vec![0; 1024]),
        Ok(())
    );
    assert_eq!(runtime.next_action(), None);
}

#[test]
fn configured_9p_servers_are_connected() {
    let mut backend = BrowserNineP::new(NinePEndpointId(7), "workspace".into());
    assert_eq!(
        backend.next_transport_action(),
        Some(NinePTransportAction::Open {
            endpoint: NinePEndpointId(7),
            generation: NinePGeneration(1),
            server_key: "workspace".into(),
        })
    );
    let old_message = [7, 0, 0, 0, 100, 1, 0];
    backend.submit(NinePRequestId(9), old_message.to_vec(), 4096);
    backend.reset(NinePGeneration(2));
    assert_eq!(
        backend.next_transport_action(),
        Some(NinePTransportAction::Close {
            endpoint: NinePEndpointId(7),
            generation: NinePGeneration(1),
        })
    );
    assert_eq!(
        backend.next_transport_action(),
        Some(NinePTransportAction::Open {
            endpoint: NinePEndpointId(7),
            generation: NinePGeneration(2),
            server_key: "workspace".into(),
        })
    );
    let message = [7, 0, 0, 0, 100, 1, 0];
    backend.submit(NinePRequestId(1), message.to_vec(), 4096);
    assert_eq!(
        backend.next_transport_action(),
        Some(NinePTransportAction::Request {
            endpoint: NinePEndpointId(7),
            generation: NinePGeneration(2),
            request_id: NinePRequestId(1),
            bytes: message.to_vec(),
            reply_capacity: 4096,
        })
    );

    let mut runtime = BrowserRuntime::default();
    runtime.start(start()).expect("start");
    let (config_id, _) = request(&mut runtime);
    runtime
        .complete_http(
            config_id,
            200,
            br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"uart",fs0:{server:"workspace",tag:"shared"},fs1:{server:"workspace",tag:"peer"}}"#.to_vec(),
        )
        .expect("JavaScript 9p configuration");
    let (firmware_id, _) = request(&mut runtime);
    runtime
        .complete_http(firmware_id, 200, vec![0; 64])
        .expect("firmware");
    assert!(runtime.is_running());
    assert_eq!(
        runtime.next_action(),
        Some(HostAction::NineP(NinePTransportAction::Open {
            endpoint: NinePEndpointId(1),
            generation: NinePGeneration(1),
            server_key: "workspace".into(),
        }))
    );
    assert_eq!(
        runtime.next_action(),
        Some(HostAction::NineP(NinePTransportAction::Open {
            endpoint: NinePEndpointId(2),
            generation: NinePGeneration(1),
            server_key: "workspace".into(),
        }))
    );
    assert_eq!(runtime.next_action(), Some(HostAction::Started));
    assert_eq!(runtime.next_action(), None);
    runtime.reset().expect("reset 9p transport");
    assert_eq!(
        runtime.next_action(),
        Some(HostAction::Reset(LifecycleCause::HostReset))
    );
    for endpoint in [1, 2] {
        assert_eq!(
            runtime.next_action(),
            Some(HostAction::NineP(NinePTransportAction::Close {
                endpoint: NinePEndpointId(endpoint),
                generation: NinePGeneration(1),
            }))
        );
        assert_eq!(
            runtime.next_action(),
            Some(HostAction::NineP(NinePTransportAction::Open {
                endpoint: NinePEndpointId(endpoint),
                generation: NinePGeneration(2),
                server_key: "workspace".into(),
            }))
        );
    }
}

#[test]
fn framebuffer_updates_coexist_with_the_virtio_console() {
    let mut runtime = BrowserRuntime::default();
    runtime.start(start()).expect("start");
    let (config_id, _) = request(&mut runtime);
    runtime
        .complete_http(
            config_id,
            200,
            br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"virtio",display0:{device:"simplefb",width:64,height:32}}"#.to_vec(),
        )
        .expect("configuration");
    let (firmware_id, _) = request(&mut runtime);
    let instructions = [0x0410_00b7_u32, 0x0010_0113, 0x0020_a023, 0x0000_006f];
    let firmware = instructions
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    runtime
        .complete_http(firmware_id, 200, firmware)
        .expect("firmware");
    assert_eq!(runtime.next_action(), Some(HostAction::Started));
    assert_eq!(runtime.next_action(), None);

    runtime
        .run_cpu_once(&mut BrowserInputQueue::default(), 0, 0, 3_000_000)
        .expect("execution slice");
    let Some(HostAction::Framebuffer(update)) = runtime.next_action() else {
        panic!("expected framebuffer action");
    };
    assert_eq!(
        (
            update.x,
            update.y,
            update.width,
            update.height,
            update.stride
        ),
        (0, 0, 64, 16, 256)
    );
    assert_eq!(
        runtime.framebuffer_bytes(update).expect("pixel rows")[..4],
        [1, 0, 0, 0]
    );
    assert_eq!(runtime.next_action(), None);
}

#[test]
fn wfi_sleep_uses_bounded_wakeup_delay() {
    let mut runtime = BrowserRuntime::default();
    runtime.start(start()).expect("start");
    let (config_id, _) = request(&mut runtime);
    runtime
        .complete_http(
            config_id,
            200,
            br#"{version:1,machine:"riscv64",memory_size:32,bios:"fw.bin",console:"uart"}"#
                .to_vec(),
        )
        .expect("configuration");
    let (firmware_id, _) = request(&mut runtime);
    runtime
        .complete_http(firmware_id, 200, 0x1050_0073_u32.to_le_bytes().to_vec())
        .expect("WFI firmware");
    assert_eq!(runtime.next_action(), Some(HostAction::Started));
    assert_eq!(runtime.next_action(), None);

    assert_eq!(runtime.begin_quantum(1_000_000), QuantumStart::Ready);
    let outcome = runtime
        .run_quantum(&mut BrowserInputQueue::default())
        .expect("waiting quantum");
    assert_eq!(outcome, QuantumOutcome::WfiSleep);
    assert_eq!(runtime.finish_quantum(1.0, 1_000_001).expect("finish"), 99);
    assert_eq!(runtime.next_action(), None);
}
