use riscbox::browser_input::BrowserInputQueue;
use riscbox::browser_runtime::{
    BrowserNineP, BrowserRuntime, HostAction, QuantumOutcome, QuantumStart, RuntimeError,
    RuntimeStart,
};
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
        .configure_quantum(1.0, 0.20, true)
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
    assert_eq!(runtime.begin_quantum(1_000_002), QuantumStart::Ready);
    runtime.abort_quantum();
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
    let (_, url) = request(&mut runtime);
    assert_eq!(url, "https://host/vm/disk/blk000000001.bin");
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
