use super::{Error, FilesystemAbi, Handle, close};
use crate::browser_runtime::BrowserRuntime;
use crate::ninep::{FilesystemError, InodeKind};

fn configuration() -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [1024_u32, 4096, 64, 64] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(100_u64.to_le_bytes());
    bytes
}

fn text(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend(u32::try_from(value.len()).unwrap().to_le_bytes());
    bytes.extend(value.as_bytes());
}

fn packet(code: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = code.to_le_bytes().to_vec();
    bytes.extend(200_u64.to_le_bytes());
    bytes.extend(0x0123_4567_89ab_cdef_u64.to_le_bytes());
    bytes.extend(body);
    bytes
}

fn invoke(abi: &mut FilesystemAbi, handle: Handle, code: u32, body: &[u8]) -> Result<i32, Error> {
    abi.output.clear();
    abi.call(handle, &packet(code, body))
}

fn path(value: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    text(&mut bytes, value);
    bytes
}

fn write(abi: &mut FilesystemAbi, handle: Handle, name: &str, value: &str) {
    let mut body = path(name);
    text(&mut body, value);
    assert_eq!(invoke(abi, handle, 2, &body), Ok(0));
}

fn lazy(abi: &mut FilesystemAbi, handle: Handle) {
    let mut body = 1_u32.to_le_bytes().to_vec();
    text(&mut body, "lazy");
    body.extend(2_u32.to_le_bytes());
    body.extend(3_u32.to_le_bytes());
    body.extend(7_u32.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    assert_eq!(invoke(abi, handle, 14, &body), Ok(0));
}

#[test]
fn host_packets_validate_before_mutation_and_enforce_limits() {
    let mut abi = FilesystemAbi::default();
    let handle = abi.create(&configuration()).unwrap();
    write(&mut abi, handle, "name", "old");
    let mut body = path("name");
    text(&mut body, "new");
    let complete = packet(2, &body);
    for end in 0..complete.len() {
        assert_eq!(abi.call(handle, &complete[..end]), Err(Error::Packet));
    }
    let mut extra = complete;
    extra.push(0);
    assert_eq!(abi.call(handle, &extra), Err(Error::Packet));
    assert_eq!(invoke(&mut abi, handle, 1, &path("name")), Ok(0));
    assert_eq!(abi.output, b"old");
    let mut oversize = path("name");
    text(&mut oversize, &"x".repeat(1025));
    assert_eq!(
        invoke(&mut abi, handle, 2, &oversize),
        Err(Error::Filesystem(FilesystemError::FileTooLarge))
    );
    assert_eq!(
        invoke(&mut abi, Handle(999), 12, &[]),
        Err(Error::BadHandle)
    );
    assert_eq!(
        invoke(&mut abi, handle, 13, &2_u32.to_le_bytes()),
        Err(Error::Packet)
    );
    assert_eq!(invoke(&mut abi, handle, 99, &[]), Err(Error::Packet));
    let tree = &abi.entries[&handle].tree;
    tree.with_filesystem(|fs| {
        let inode = fs.inode(fs.lookup("name").unwrap()).unwrap();
        assert_eq!(inode.mtime, 200);
        assert!(matches!(inode.kind, InodeKind::File(_)));
    });
}

#[test]
fn lazy_host_reads_pin_inodes_and_survive_path_reuse() {
    let mut abi = FilesystemAbi::default();
    let handle = abi.create(&configuration()).unwrap();
    lazy(&mut abi, handle);
    assert_eq!(invoke(&mut abi, handle, 1, &path("lazy")), Ok(1));
    let request = u32::from_le_bytes(abi.output[..4].try_into().unwrap());
    let tree = abi.entries[&handle].tree.clone();
    let load = tree.next_load().unwrap();
    assert_eq!(invoke(&mut abi, handle, 1, &path("lazy")), Ok(1));
    let joined = u32::from_le_bytes(abi.output[..4].try_into().unwrap());
    assert!(tree.next_load().is_none());
    assert_eq!(invoke(&mut abi, handle, 15, &request.to_le_bytes()), Ok(1));
    assert_eq!(invoke(&mut abi, handle, 4, &path("lazy")), Ok(0));
    write(&mut abi, handle, "lazy", "replacement");
    tree.with_filesystem(|fs| fs.complete_load(load, b"old".to_vec()))
        .unwrap();
    assert_eq!(invoke(&mut abi, handle, 15, &request.to_le_bytes()), Ok(0));
    assert_eq!(abi.output, b"old");
    assert_eq!(invoke(&mut abi, handle, 15, &joined.to_le_bytes()), Ok(0));
    assert_eq!(abi.output, b"old");
    assert!(tree.with_filesystem(|fs| fs.inode(load.inode).is_none()));
    assert_eq!(
        invoke(&mut abi, handle, 15, &request.to_le_bytes()),
        Err(Error::BadHandle)
    );
}

#[test]
fn host_read_cancellation_reset_and_failure_release_retained_state() {
    let mut abi = FilesystemAbi::default();
    let handle = abi.create(&configuration()).unwrap();
    lazy(&mut abi, handle);
    assert_eq!(invoke(&mut abi, handle, 1, &path("lazy")), Ok(1));
    let request = u32::from_le_bytes(abi.output[..4].try_into().unwrap());
    let tree = abi.entries[&handle].tree.clone();
    let load = tree.next_load().unwrap();
    assert_eq!(invoke(&mut abi, handle, 12, &[]), Ok(0));
    assert_eq!(
        invoke(&mut abi, handle, 15, &request.to_le_bytes()),
        Err(Error::Stale)
    );
    assert!(
        tree.with_filesystem(|fs| fs.complete_load(load, b"old".to_vec()))
            .is_err()
    );
    lazy(&mut abi, handle);
    assert_eq!(invoke(&mut abi, handle, 1, &path("lazy")), Ok(1));
    let request = u32::from_le_bytes(abi.output[..4].try_into().unwrap());
    let load = tree.next_load().unwrap();
    assert!(
        tree.with_filesystem(|fs| fs.complete_load(load, b"bad length".to_vec()))
            .is_err()
    );
    assert_eq!(
        invoke(&mut abi, handle, 15, &request.to_le_bytes()),
        Err(Error::Filesystem(FilesystemError::LoadFailed))
    );
    assert_eq!(invoke(&mut abi, handle, 17, &path("lazy")), Ok(0));
    assert_eq!(invoke(&mut abi, handle, 1, &path("lazy")), Ok(1));
    let request = u32::from_le_bytes(abi.output[..4].try_into().unwrap());
    assert_eq!(invoke(&mut abi, handle, 16, &request.to_le_bytes()), Ok(0));
    assert_eq!(
        invoke(&mut abi, handle, 16, &request.to_le_bytes()),
        Err(Error::BadHandle)
    );
    assert!(abi.entries[&handle].reads.is_empty());
    let attachment = tree.attach().unwrap();
    assert_eq!(
        close(&mut abi, &mut BrowserRuntime::default(), handle),
        Err(Error::Busy)
    );
    drop(attachment);
    assert_eq!(
        close(&mut abi, &mut BrowserRuntime::default(), handle),
        Ok(0)
    );
    let next = abi.create(&configuration()).unwrap();
    assert_ne!(handle, next);
}

#[test]
fn seed_replacement_is_atomic_and_preserves_full_timestamp_widths() {
    let mut abi = FilesystemAbi::default();
    let handle = abi.create(&configuration()).unwrap();
    write(&mut abi, handle, "before", "retained");
    let mut body = 1_u32.to_le_bytes().to_vec();
    text(&mut body, "file");
    body.extend(2_u32.to_le_bytes());
    body.extend(3_u32.to_le_bytes());
    body.extend(7_u32.to_le_bytes());
    body.extend(16_u32.to_le_bytes());
    body.extend(u64::MAX.to_le_bytes());
    let mut invalid = body.clone();
    invalid.push(0);
    assert_eq!(invoke(&mut abi, handle, 14, &invalid), Err(Error::Packet));
    assert_eq!(invoke(&mut abi, handle, 1, &path("before")), Ok(0));
    assert_eq!(abi.output, b"retained");
    assert_eq!(invoke(&mut abi, handle, 14, &body), Ok(0));
    assert_eq!(invoke(&mut abi, handle, 8, &path("file")), Ok(0));
    assert_eq!(
        u64::from_le_bytes(abi.output[52..60].try_into().unwrap()),
        u64::MAX
    );
}
