use riscbox::config::{
    Console, DriveConfig, NetworkDriver, Value, VmConfig, parse_value, resolve_asset_path,
};

const COMPLETE: &str = r#"
{
    // deployed files use identifiers and trailing commas
    version: 1,
    machine: "riscv64",
    memory_size: 0x100,
    bios: "fw_dynamic.bin",
    kernel: "linux",
    initrd: "initrd.img",
    cmdline: "root=/dev/vda",
    console: "uart",
    uart_output: true,
    drive0: { file: "drive/blk.txt", device: "virtio" },
    fs0: { server: "workspace", tag: "shared" },
    eth0: { driver: "user" },
    display0: { device: "simplefb", width: 640, height: 480 },
    input_device: "virtio",
    rtc_local_time: true,
}
"#;

#[test]
fn parses_deployed_syntax_and_complete_schema() {
    let config = VmConfig::parse(COMPLETE).expect("complete config");
    assert_eq!(config.machine, "riscv64");
    assert_eq!(config.memory_size_mib, 256);
    assert_eq!(config.console, Console::Uart);
    assert!(config.uart_output);
    assert!(matches!(&config.drives[0], DriveConfig::Http { file, .. } if file == "drive/blk.txt"));
    assert_eq!(config.filesystems[0].server, "workspace");
    assert_eq!(config.filesystems[0].tag, "shared");
    assert_eq!(config.networks[0].driver, NetworkDriver::User);
    assert_eq!(config.display.expect("display").width, 640);
    assert!(config.rtc_local_time);
}

#[test]
fn pseudo_json_value_api_handles_arrays_comments_and_escapes() {
    let value = parse_value(r#"/*x*/ { list: [01, 0x10, "a\n\x42",], flag: false, }"#)
        .expect("pseudo JSON");
    let object = value.as_object().expect("object");
    assert_eq!(
        object["list"].as_array().expect("array"),
        &[
            Value::Integer(1),
            Value::Integer(16),
            Value::String("a\nB".to_owned())
        ]
    );
    assert_eq!(object["flag"], Value::Bool(false));
    assert!(parse_value("-1").is_err());
    assert!(parse_value("{} trailing").is_err());
    assert!(parse_value("/* unterminated").is_err());
    assert_eq!(
        parse_value(r#""\u00e9\uD83D\uDE00\b\/""#).expect("JSON string"),
        Value::String("é😀\u{0008}/".into())
    );
    assert!(parse_value(r#""\uD83D""#).is_err());
}

#[test]
fn applies_defaults_and_reports_schema_errors() {
    let mut config =
        VmConfig::parse("{version:1,machine:\"riscv64\",memory_size:128}").expect("minimal config");
    assert_eq!(config.console, Console::Virtio);
    assert!(!config.uart_output);
    assert!(config.drives.is_empty());
    config.apply_command_line("console=hvc0");
    assert_eq!(config.command_line.as_deref(), Some(" console=hvc0"));
    config.apply_command_line("!console=ttyS0");
    assert_eq!(config.command_line.as_deref(), Some("console=ttyS0"));

    for invalid in [
        "{}",
        "{version:2,machine:\"riscv64\",memory_size:128}",
        "{version:1,machine:\"riscv64\",memory_size:\"128\"}",
        "{version:1,machine:\"riscv64\",memory_size:128,console:\"bad\"}",
        "{version:1,machine:\"riscv64\",memory_size:128,uart_output:1}",
        "{version:1,machine:\"riscv64\",memory_size:128,fs0:{js9p:true,file:\"x\"}}",
        "{version:1,machine:\"riscv64\",memory_size:128,fs0:{js9p:true,tag:\"x\"}}",
        "{version:1,machine:\"riscv64\",memory_size:128,fs0:{server:\"x\"}}",
        "{version:1,machine:\"riscv64\",memory_size:128,fs0:{server:\"\",tag:\"x\"}}",
        "{version:1,machine:\"riscv64\",memory_size:128,eth0:{driver:\"tap\"}}",
        "{version:1,machine:\"riscv64\",memory_size:128,eth0:{driver:\"user\",ifname:\"tap0\"}}",
    ] {
        assert!(VmConfig::parse(invalid).is_err(), "accepted {invalid}");
    }
}

#[test]
fn parses_full_width_boot_addresses() {
    let config = VmConfig::parse(
        "{version:1,machine:\"riscv64\",memory_size:128,kernel:\"Image\",kernel_address:\"0x100000000\",fdt_address:4096}"
    ).expect("boot addresses");
    assert_eq!(config.kernel_address, Some(0x1_0000_0000));
    assert_eq!(config.fdt_address, Some(4096));
    assert!(config.bios.is_none());
    for value in ["-1", "\"nope\"", "true"] {
        let source =
            format!("{{version:1,machine:\"riscv64\",memory_size:128,kernel_address:{value}}}");
        assert!(VmConfig::parse(&source).is_err(), "accepted {value}");
    }
}

#[test]
fn numbered_entries_stop_at_gaps_and_enforce_limits() {
    let config = VmConfig::parse(
        "{version:1,machine:\"riscv64\",memory_size:128,drive1:{file:\"ignored\"}}",
    )
    .expect("gap is accepted");
    assert!(config.drives.is_empty());

    let too_many = "{version:1,machine:\"riscv64\",memory_size:128,drive0:{file:\"0\"},drive1:{file:\"1\"},drive2:{file:\"2\"},drive3:{file:\"3\"},drive4:{file:\"4\"}}";
    assert!(VmConfig::parse(too_many).is_err());
}

#[test]
fn resolves_assets_relative_to_the_configuration() {
    assert_eq!(
        resolve_asset_path(Some("https://host/vm/riscbox.cfg"), "fw.bin"),
        "https://host/vm/fw.bin"
    );
    assert_eq!(
        resolve_asset_path(Some("https://host/vm/riscbox.cfg"), "data:image/raw"),
        "data:image/raw"
    );
    assert_eq!(
        resolve_asset_path(Some("https://host/vm/riscbox.cfg"), "/fw.bin"),
        "/fw.bin"
    );
    assert_eq!(resolve_asset_path(Some("riscbox.cfg"), "fw.bin"), "fw.bin");
    assert_eq!(resolve_asset_path(None, "fw.bin"), "fw.bin");
}

#[test]
fn resolved_config_requires_explicit_defaults_and_validates_devices() {
    let source = r#"{"version":1,"machine":"riscv64","memory_size":128,
        "console":"virtio","uart_output":false,"rtc_local_time":false,
        "cmdline":"","drive0":{"file":"https://host/disk/blk.txt"}}"#;
    let config = VmConfig::from_resolved(source).expect("resolved config");
    assert!(
        matches!(&config.drives[0], DriveConfig::Http { file, .. } if file == "https://host/disk/blk.txt")
    );
    assert!(
        VmConfig::from_resolved(r#"{"version":1,"machine":"riscv64","memory_size":128}"#).is_err()
    );
    assert!(VmConfig::from_resolved(&source.replace("riscv64", "other")).is_err());
    assert!(VmConfig::from_resolved(&source.replace("https://host/disk/blk.txt", "")).is_err());
}

#[test]
fn resolved_host_drives_keep_order_and_validate_capacity() {
    let source = r#"{"version":1,"machine":"riscv64","memory_size":128,
        "console":"virtio","uart_output":false,"rtc_local_time":false,"cmdline":"",
        "drive0":{"provider":7,"capacity_sectors":"4294967296"},
        "drive1":{"file":"https://host/disk/blk.txt"}}"#;
    let config = VmConfig::from_resolved(source).expect("mixed drives");
    assert_eq!(
        config.drives[0],
        DriveConfig::Host {
            provider: 7,
            capacity_sectors: 4_294_967_296
        }
    );
    assert!(matches!(config.drives[1], DriveConfig::Http { .. }));
    for bad in [
        source.replace("\"4294967296\"", "\"0\""),
        source.replace("\"4294967296\"", "\"nope\""),
        source.replace("\"provider\":7", "\"provider\":0"),
        source.replace("\"provider\":7", "\"provider\":7,\"file\":\"x\""),
        source.replace(
            "\"drive1\":{\"file\":\"https://host/disk/blk.txt\"}",
            "\"drive1\":{\"provider\":7,\"capacity_sectors\":\"8\"}",
        ),
    ] {
        assert!(VmConfig::from_resolved(&bad).is_err(), "accepted {bad}");
    }
}
