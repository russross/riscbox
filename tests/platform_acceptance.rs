use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use riscbox::machine::{BootImages, Machine, MachineConfig};
use riscbox::ninep::{Filesystem, Limits};
use riscbox::ninep_backend::{RustFilesystem, RustNineP};
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
#[ignore = "requires a RISC-V Alpine standard ISO and built OpenSBI/U-Boot assets"]
fn alpine_iso_boots_through_efi_and_shuts_down() {
    let iso = read(&required_path("RISCBOX_ALPINE_ISO"));
    let firmware = read(Path::new("opensbi/fw_dynamic.bin"));
    let bootloader = read(Path::new("uboot/u-boot.bin"));
    let mut machine = Machine::new(MachineConfig {
        ram_size: 512 << 20,
        framebuffer: None,
    })
    .expect("ISO machine");

    // The ISO uses the same whole-sector block transport as ordinary disks.
    // Writes remain private to this test's in-memory copy of the media.
    machine
        .add_block_device(
            Box::new(ImageBlock { bytes: iso }),
            *b"riscbox-iso-disk-000",
        )
        .expect("ISO block device");
    machine
        .load_boot(BootImages {
            firmware: Some(&firmware),
            kernel: Some(&bootloader),
            initrd: None,
            command_line: "",
        })
        .expect("ISO boot images");

    // Follow the unmodified media through EFI, GRUB, Linux, and live userspace.
    // UART commands check both the ISO tree and its embedded FAT boot image.
    let mut transcript = Vec::new();
    let mut logged_in = false;
    let mut command_sent = false;
    let mut pending = Vec::new();
    let mut submitted = 0;
    for batch in 0..20_000_u64 {
        let ticks = batch * 100_000;
        machine.present_guest_clocks(ticks, ticks * 100);
        for _ in 0..15 {
            submitted += machine.receive_console(&pending[submitted..]);
            let _ = machine.run_cpu(200_000);
        }
        let output = machine.take_console_output();
        trace_guest_output(&output);
        transcript.extend(output);
        let text = String::from_utf8_lossy(&transcript);
        if !logged_in && text.contains("login:") {
            assert_eq!(machine.receive_console(b"root\r"), 5);
            logged_in = true;
        }
        if logged_in && !command_sent && text.contains(":~#") {
            let command = concat!(
                "mkdir -p /mnt/iso /mnt/fat; ",
                "mount -t iso9660 -o ro /dev/vda /mnt/iso && ",
                "test -s /mnt/iso/boot/grub/grub.cfg && ",
                "mount -t vfat -o ro,loop /mnt/iso/boot/grub/efi.img /mnt/fat && ",
                "test -s /mnt/fat/efi/boot/bootriscv64.efi && ",
                "echo ISO_BOOT_OK; poweroff -f\r"
            );
            pending.extend_from_slice(command.as_bytes());
            command_sent = true;
        }

        // A completed shutdown must follow successful filesystem checks.
        if machine.finish_status() != riscbox::platform::FinishStatus::Running {
            assert!(command_sent, "ISO guest stopped before login\n{text}");
            assert!(
                text.lines().any(|line| line.trim() == "ISO_BOOT_OK"),
                "ISO/FAT checks failed\n{text}"
            );
            return;
        }
        assert!(!text.contains("Kernel panic"), "ISO guest panic\n{text}");
    }
    panic!(
        "ISO acceptance timed out\n{}",
        String::from_utf8_lossy(&transcript)
    );
}

#[test]
#[ignore = "requires ignored Alpine deployment assets and runs a complete guest"]
fn alpine_reaches_login_and_shuts_down() {
    let directory = acceptance_directory();
    let firmware = read(&directory.join("fw_dynamic.bin"));
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
    let mut filesystem = Filesystem::new(Limits::default(), 100);
    filesystem.write_file("host", b"seeded").expect("host file");
    filesystem
        .write_file("resident", b"ready")
        .expect("resident file");
    let share = RustFilesystem::new(filesystem);
    machine
        .add_ninep_device(
            Box::new(RustNineP::new(share.clone()).expect("Rust session")),
            b"shared",
        )
        .expect("9p device");
    machine
        .load_boot(BootImages {
            firmware: Some(&firmware),
            kernel: Some(&kernel),
            initrd: None,
            command_line: "root=/dev/vda rw rootfstype=ext4 console=hvc0 earlycon=sbi",
        })
        .expect("boot images");

    let mut transcript = Vec::new();
    let mut logged_in = false;
    let mut shutdown_sent = false;
    for batch in 0..10_000_u64 {
        let ticks = batch * 10_000;
        machine.present_guest_clocks(ticks, ticks * 100);
        for _ in 0..15 {
            let _ = machine.run_cpu(200_000);
        }
        let output = machine.take_console_output();
        trace_guest_output(&output);
        transcript.extend(output);
        let virtio_output = machine
            .take_virtio_console_output(console)
            .expect("VirtIO console output");
        trace_guest_output(&virtio_output);
        transcript.extend(virtio_output);
        let text = String::from_utf8_lossy(&transcript);
        if !logged_in && text.contains("login:") {
            machine
                .virtio_console_receive(console, b"root\r")
                .expect("login input");
            logged_in = true;
        }
        if logged_in && !shutdown_sent && text.contains(":~#") {
            machine
                .virtio_console_receive(console, concat!(
                    "mkdir -p /mnt/share; mount -t 9p -o trans=virtio,version=9p2000.L,access=client shared /mnt/share",
                    " && [ \"$(cat /mnt/share/host)\" = seeded ] && [ \"$(cat /mnt/share/resident)\" = ready ]",
                    " && printf written > /mnt/share/guest && ln /mnt/share/guest /mnt/share/link",
                    " && mv /mnt/share/link /mnt/share/renamed && chmod 640 /mnt/share/guest",
                    " && mkdir /mnt/share/dir && ln -s guest /mnt/share/symlink",
                    " && [ \"$(cat /mnt/share/symlink)\" = written ] && rm /mnt/share/symlink",
                    " && rmdir /mnt/share/dir && ls /mnt/share >/dev/null && sync && echo RUST9P_OK; poweroff -f\r",
                ).as_bytes())
                .expect("shutdown input");
            shutdown_sent = true;
        }
        if machine.finish_status() != riscbox::platform::FinishStatus::Running {
            assert!(logged_in, "guest shut down before login\n{text}");
            assert!(shutdown_sent, "guest shut down before command\n{text}");
            assert!(
                text.lines().any(|line| line.trim() == "RUST9P_OK"),
                "Linux 9p operations failed\n{text}"
            );
            assert_eq!(
                share
                    .with_filesystem(|fs| fs.read_file("guest"))
                    .expect("guest file"),
                b"written".to_vec()
            );
            return;
        }
    }
    panic!(
        "Alpine acceptance timed out\n{}",
        String::from_utf8_lossy(&transcript)
    );
}

#[test]
#[ignore = "requires built OpenSBI, Linux, and Risclet EROFS assets"]
fn risclet_direct_kernel_reaches_linux_userspace() {
    let firmware = read(Path::new("opensbi/fw_dynamic.bin"));
    let kernel = read(Path::new("kernel/linux"));
    let disk = read(Path::new("images/risclet/build/rootfs.erofs"));
    let mut machine = Machine::new(MachineConfig {
        ram_size: 256 << 20,
        framebuffer: None,
    })
    .expect("Risclet machine");
    machine
        .add_block_device(
            Box::new(ImageBlock { bytes: disk }),
            *b"riscbox-http-disk-00",
        )
        .expect("Risclet block device");
    let console = machine.add_console_device(80, 25).expect("VirtIO console");
    machine
        .load_boot(BootImages {
            firmware: Some(&firmware),
            kernel: Some(&kernel),
            initrd: None,
            command_line: "root=/dev/vda ro rootfstype=erofs console=hvc0 quiet loglevel=0",
        })
        .expect("OpenSBI and Linux images");

    // The image's init script attempts a 9p mount after Linux mounts EROFS.
    // No 9p backend is installed here, so that error marks userspace startup.
    let mut transcript = Vec::new();
    for batch in 0..10_000_u64 {
        let ticks = batch * 10_000;
        machine.present_guest_clocks(ticks, ticks * 100);
        for _ in 0..15 {
            let _ = machine.run_cpu(200_000);
        }
        let output = machine.take_console_output();
        trace_guest_output(&output);
        transcript.extend(output);
        let output = machine
            .take_virtio_console_output(console)
            .expect("VirtIO console output");
        trace_guest_output(&output);
        transcript.extend(output);
        let text = String::from_utf8_lossy(&transcript);
        if text.contains("mounting risclet") {
            return;
        }
        assert!(!text.contains("Kernel panic"), "Risclet panic\n{text}");
    }
    panic!(
        "Risclet direct kernel boot timed out\n{}",
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
            firmware: Some(&kernel),
            kernel: None,
            initrd: None,
            command_line: "",
        })
        .expect("xv6 image");

    let mut transcript = Vec::new();
    let mut tests_started = false;
    for batch in 0..20_000_u64 {
        let ticks = batch * 1_000_000;
        machine.present_guest_clocks(ticks, ticks * 100);
        for _ in 0..15 {
            let _ = machine.run_cpu(200_000);
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
