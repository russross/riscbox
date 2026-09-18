use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use riscbox::machine::{BootImages, Machine, MachineConfig};
use riscbox::virtio_devices::{BlockBackend, DeviceError};

struct ImageBlock {
    bytes: Vec<u8>,
}

impl BlockBackend for ImageBlock {
    fn capacity_sectors(&self) -> u64 {
        u64::try_from(self.bytes.len() / 512).expect("test image size fits u64")
    }

    fn read(&mut self, sector: u64, data: &mut [u8]) -> Result<(), DeviceError> {
        let start = usize::try_from(sector)
            .ok()
            .and_then(|value| value.checked_mul(512))
            .ok_or(DeviceError::Backend)?;
        let end = start.checked_add(data.len()).ok_or(DeviceError::Backend)?;
        let source = self.bytes.get(start..end).ok_or(DeviceError::Backend)?;
        data.copy_from_slice(source);
        Ok(())
    }

    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), DeviceError> {
        let start = usize::try_from(sector)
            .ok()
            .and_then(|value| value.checked_mul(512))
            .ok_or(DeviceError::Backend)?;
        let end = start.checked_add(data.len()).ok_or(DeviceError::Backend)?;
        let target = self.bytes.get_mut(start..end).ok_or(DeviceError::Backend)?;
        target.copy_from_slice(data);
        Ok(())
    }
}

#[test]
#[ignore = "requires ignored Alpine deployment assets and runs a complete guest"]
fn alpine_reaches_login_and_shuts_down() {
    let directory = acceptance_directory();
    let firmware = read(&directory.join("fw_jump.bin"));
    let kernel = read(&directory.join("linux"));
    let disk = read(&directory.join("rootfs.ext4"));
    let mut machine = Machine::new(MachineConfig {
        ram_size: 256 << 20,
        framebuffer: None,
    })
    .expect("acceptance machine");
    machine
        .add_block_device(
            Box::new(ImageBlock { bytes: disk }),
            *b"riscbox-http-disk-00",
        )
        .expect("block device");
    let console = machine.add_console_device(80, 25).expect("console device");
    machine
        .load_boot(BootImages {
            firmware: &firmware,
            kernel: Some(&kernel),
            initrd: None,
            command_line: "root=/dev/vda rw rootfstype=ext4 console=hvc0 earlycon=sbi",
        })
        .expect("boot images");

    let mut transcript = Vec::new();
    let mut logged_in = false;
    let mut shutdown_sent = false;
    for slice in 0..10_000_u64 {
        let ticks = slice * 10_000;
        machine.update_time(ticks, ticks * 100);
        for _ in 0..15 {
            let _ = machine.run(200_000);
        }
        let output = machine.take_console_output();
        trace_guest_output(&output);
        transcript.extend(output);
        transcript.extend(
            machine
                .take_virtio_console_output(console)
                .expect("VirtIO console output"),
        );
        let text = String::from_utf8_lossy(&transcript);
        if !logged_in && text.contains("login:") {
            machine
                .virtio_console_receive(console, b"root\r")
                .expect("login input");
            logged_in = true;
        }
        if logged_in && !shutdown_sent && text.contains(":~#") {
            machine
                .virtio_console_receive(console, b"poweroff -f\r")
                .expect("shutdown input");
            shutdown_sent = true;
        }
        if machine.finish_status() != riscbox::platform::FinishStatus::Running {
            assert!(logged_in, "guest shut down before login\n{text}");
            assert!(shutdown_sent, "guest shut down before command\n{text}");
            return;
        }
    }
    panic!(
        "Alpine acceptance timed out\n{}",
        String::from_utf8_lossy(&transcript)
    );
}

#[test]
#[ignore = "requires a flat xv6 kernel and fs.img and runs the complete user suite"]
fn xv6_boots_over_uart_and_passes_user_tests() {
    let kernel_path = required_path("RISCBOX_XV6_KERNEL");
    let disk_path = required_path("RISCBOX_XV6_DISK");
    let kernel = read(&kernel_path);
    let disk = read(&disk_path);
    let mut machine = Machine::new(MachineConfig {
        ram_size: 128 << 20,
        framebuffer: None,
    })
    .expect("xv6 machine");
    machine
        .add_block_device(
            Box::new(ImageBlock { bytes: disk }),
            *b"riscbox-xv6-disk-000",
        )
        .expect("xv6 block device");
    machine
        .load_boot(BootImages {
            firmware: &kernel,
            kernel: None,
            initrd: None,
            command_line: "",
        })
        .expect("xv6 image");

    let mut transcript = Vec::new();
    let mut tests_started = false;
    for slice in 0..20_000_u64 {
        let ticks = slice * 1_000_000;
        machine.update_time(ticks, ticks * 100);
        for _ in 0..15 {
            let _ = machine.run(200_000);
        }
        let output = machine.take_console_output();
        trace_guest_output(&output);
        transcript.extend(output);
        let text = String::from_utf8_lossy(&transcript);
        if !tests_started && text.contains("$ ") {
            let accepted = machine.receive_console(b"usertests -q\r");
            assert_eq!(accepted, 13, "UART accepted complete command");
            tests_started = true;
        }
        if text.contains("ALL TESTS PASSED") {
            return;
        }
        assert!(!text.contains("panic:"), "xv6 panic\n{text}");
    }
    panic!(
        "xv6 acceptance timed out\n{}",
        String::from_utf8_lossy(&transcript)
    );
}

fn acceptance_directory() -> PathBuf {
    std::env::var_os("RISCBOX_ALPINE_DIR")
        .map_or_else(|| PathBuf::from("images/alpine/dist"), PathBuf::from)
}

fn required_path(variable: &str) -> PathBuf {
    std::env::var_os(variable).map_or_else(
        || panic!("{variable} must name an acceptance-test asset"),
        PathBuf::from,
    )
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
}

fn trace_guest_output(output: &[u8]) {
    if std::env::var_os("RISCBOX_ACCEPTANCE_TRACE").is_some() {
        print!("{}", String::from_utf8_lossy(output));
        io::stdout().flush().expect("acceptance trace flush");
    }
}
