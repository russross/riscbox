//! Standalone raw-WASM namespace probe, with no WASI or JavaScript imports.

pub use riscbox::{ninep, ninep_protocol};

mod ninep_transport;

use ninep::{ChangeKind, FileRead, Filesystem, Limits, LoadStart, SeedMetadata, SourceId};
use ninep_protocol::{Completion, NinePSession, Outcome, RequestId, Submission};
use std::sync::atomic::{AtomicU32, Ordering};

static PANIC_LINE: AtomicU32 = AtomicU32::new(0);

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

// Exporting a real operation prevents dead-code elimination from concealing
// unsupported standard-library calls in the production WASM target.
#[unsafe(no_mangle)]
pub extern "C" fn namespace_regression() -> u32 {
    let mut fs = Filesystem::new(Limits::default(), 100);
    let root = fs.root();
    let id = fs.add_lazy_file("file", 3, SourceId(7)).unwrap();
    fs.set_metadata(
        id,
        SeedMetadata {
            mtime: Some(12),
            ctime: Some(13),
            ..SeedMetadata::default()
        },
    )
    .unwrap();
    let load = fs.begin_load(id).unwrap();
    assert!(matches!(load, LoadStart::Started(_)));
    assert_eq!(fs.begin_load(id).unwrap(), LoadStart::Joined(load.ticket()));
    fs.set_time(200);
    fs.complete_load(load.ticket(), b"old".to_vec()).unwrap();
    assert_eq!(fs.inode(id).unwrap().mtime, 12);
    assert_eq!(fs.inode(id).unwrap().ctime, 13);

    // The host can edit and reset the tree without ever constructing a VM.
    fs.write_file("file", b"new").unwrap();
    assert_eq!(
        fs.read_file("file").unwrap(),
        FileRead::Resident(b"new".to_vec())
    );
    assert_eq!(fs.inode(id).unwrap().mtime, 200);
    fs.reset().unwrap();
    assert_ne!(fs.root(), root);
    assert!(fs.inode(id).is_none());
    assert_eq!(fs.next_change().unwrap().kind, ChangeKind::Reset);
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
    id: u64,
    kind: u8,
    tag: u16,
    body: &[u8],
) -> Vec<u8> {
    let Submission::Immediate(reply) = session
        .submit(fs, RequestId(id), &message(kind, tag, body), 8192)
        .unwrap()
    else {
        panic!("unexpected pending request");
    };
    reply
}

// Actual protocol execution exercises pointer-width-sensitive parsing, shared
// loads, cancellation, and fresh namespace generations in the browser target.
#[unsafe(no_mangle)]
pub extern "C" fn protocol_regression() -> u32 {
    let mut fs = Filesystem::new(Limits::default(), 100);
    fs.add_lazy_file("file", 3, SourceId(1)).unwrap();
    let mut session = NinePSession::new(&mut fs).unwrap();
    let mut body = 8192_u32.to_le_bytes().to_vec();
    append_string(&mut body, "9P2000.L");
    assert_eq!(
        resident(&mut session, &mut fs, 1, 100, u16::MAX, &body)[4],
        101
    );
    let mut body = 1_u32.to_le_bytes().to_vec();
    body.extend(u32::MAX.to_le_bytes());
    append_string(&mut body, "");
    append_string(&mut body, "");
    body.extend(123_u32.to_le_bytes());
    assert_eq!(resident(&mut session, &mut fs, 2, 104, 1, &body)[4], 105);
    let mut body = 1_u32.to_le_bytes().to_vec();
    body.extend(2_u32.to_le_bytes());
    body.extend(1_u16.to_le_bytes());
    append_string(&mut body, "file");
    assert_eq!(resident(&mut session, &mut fs, 3, 110, 2, &body)[4], 111);
    let mut body = 2_u32.to_le_bytes().to_vec();
    body.extend(0_u32.to_le_bytes());
    assert_eq!(resident(&mut session, &mut fs, 4, 12, 3, &body)[4], 13);

    let mut read = 2_u32.to_le_bytes().to_vec();
    read.extend(0_u64.to_le_bytes());
    read.extend(3_u32.to_le_bytes());
    let Submission::Pending(LoadStart::Started(ticket)) = session
        .submit(&mut fs, RequestId(5), &message(116, 4, &read), 100)
        .unwrap()
    else {
        panic!("expected source load");
    };
    assert_eq!(
        resident(&mut session, &mut fs, 6, 108, 5, &4_u16.to_le_bytes())[4],
        109
    );
    assert_eq!(
        session.next_completion(),
        Some(Completion {
            request: RequestId(5),
            outcome: Outcome::Suppressed
        })
    );
    assert_eq!(
        session
            .submit(&mut fs, RequestId(7), &message(116, 4, &read), 100)
            .unwrap(),
        Submission::Pending(LoadStart::Joined(ticket))
    );
    fs.complete_load(ticket, b"abc".to_vec()).unwrap();
    session.poll(&mut fs).unwrap();
    let Completion {
        request: RequestId(7),
        outcome: Outcome::Reply(reply),
    } = session.next_completion().unwrap()
    else {
        panic!("expected retained reply");
    };
    assert_eq!(reply[4], 117);
    assert_eq!(&reply[11..], b"abc");

    // A 64-bit offset remains intact on wasm32 and returns EOF without narrowing.
    read[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(resident(&mut session, &mut fs, 8, 116, 6, &read).len(), 11);
    fs.reset().unwrap();
    session.poll(&mut fs).unwrap();
    let reply = resident(&mut session, &mut fs, 9, 116, 7, &read);
    assert_eq!(reply[4], 7);
    assert_eq!(&reply[7..11], &9_u32.to_le_bytes());
    session.close(&mut fs).unwrap();
    1
}
