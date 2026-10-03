//! Standalone raw-WASM namespace probe, with no WASI or JavaScript imports.

pub use riscbox::{ninep, ninep_protocol};

mod ninep_transport;
mod cpu_hot_path;
mod pmp;

use ninep::{ChangeKind, Filesystem, Limits};
use ninep_protocol::NinePSession;
use std::sync::atomic::{AtomicU32, Ordering};

static PANIC_LINE: AtomicU32 = AtomicU32::new(0);

// The same instruction streams validate native and raw-WASM CPU execution.
#[unsafe(no_mangle)]
pub extern "C" fn cpu_hot_path_regression() -> u32 {
    cpu_hot_path::retirement();
    cpu_hot_path::retirement_handoff();
    cpu_hot_path::high_multiply();
    pmp::memory_accesses();
    pmp::csr_handlers();
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn panic_line() -> u32 {
    PANIC_LINE.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn transport_regression() -> u32 {
    std::panic::set_hook(Box::new(|panic| {
        if let Some(location) = panic.location() {
            PANIC_LINE.store(location.line(), Ordering::Relaxed);
        }
    }));
    ninep_transport::regression();
    1
}



fn message(kind: u8, tag: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(body.len() + 7)
        .unwrap()
        .to_le_bytes()
        .to_vec();
    bytes.push(kind);
    bytes.extend(tag.to_le_bytes());
    bytes.extend(body);
    bytes
}

fn append_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend(u16::try_from(value.len()).unwrap().to_le_bytes());
    bytes.extend(value.as_bytes());
}

fn resident(
    session: &mut NinePSession,
    fs: &mut Filesystem,
    kind: u8,
    tag: u16,
    body: &[u8],
) -> Vec<u8> {
    let reply = session
        .submit(fs, &message(kind, tag, body), 8192)
        .unwrap();
    reply
}



// Raw WASM executes real namespace operations without operating-system services.
#[unsafe(no_mangle)]
pub extern "C" fn namespace_regression() -> u32 {
    let mut fs = Filesystem::new(Limits::default(), 100);
    fs.mkdir("dir").unwrap();
    fs.write_file("dir/file", b"resident").unwrap();
    fs.hard_link("dir/file", "alias").unwrap();
    fs.remove("dir/file").unwrap();
    assert_eq!(fs.read_file("alias").unwrap(), b"resident");
    assert_eq!(fs.next_change().unwrap().kind, ChangeKind::Create);
    fs.reset().unwrap();
    assert!(fs.list_files().is_empty());
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn protocol_regression() -> u32 {
    let mut fs = Filesystem::new(Limits::default(), 100);
    let mut session = NinePSession::new(&mut fs).unwrap();
    let mut version = 4096_u32.to_le_bytes().to_vec();
    append_string(&mut version, "9P2000.L");
    assert_eq!(resident(&mut session, &mut fs, 100, u16::MAX, &version)[4], 101);
    assert_eq!(resident(&mut session, &mut fs, 108, 1, &9_u16.to_le_bytes())[4], 109);
    session.close(&mut fs).unwrap();
    1
}
