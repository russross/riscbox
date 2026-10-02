use super::{Entry, Error, FilesystemAbi, Handle};
use crate::ninep::{Filesystem, FilesystemError, InodeKind, Limits};
use crate::ninep_backend::RustFilesystem;

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

#[test]
fn host_packets_validate_before_mutation_and_enforce_limits() {
    let mut abi = FilesystemAbi::default();
    let handle = Handle(1);
    abi.entries.insert(
        handle,
        Entry {
            tree: RustFilesystem::new(Filesystem::new(
                Limits {
                    max_file_bytes: 1024,
                    max_tree_bytes: 4096,
                    max_inodes: 64,
                    max_directory_entries: 64,
                },
                100,
            )),
        },
    );
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

// Destruction invalidates every old reference, even if the same name returns.
#[test]
fn destroy_invalidates_host_handles() {
    let mut abi = FilesystemAbi::default();
    let handle = Handle(1);
    abi.entries.insert(
        handle,
        Entry {
            tree: RustFilesystem::new(Filesystem::new(Limits::default(), 100)),
        },
    );
    write(&mut abi, handle, "file", "resident");
    assert_eq!(invoke(&mut abi, handle, 1, &path("file")), Ok(0));
    assert_eq!(abi.output, b"resident");
    abi.destroy();
    assert_eq!(
        invoke(&mut abi, handle, 1, &path("file")),
        Err(Error::BadHandle)
    );
}

// Attribute packets validate completely before changing a live inode.
#[test]
fn restored_attributes_keep_permissions_ownership_and_timestamp_precision() {
    let mut abi = FilesystemAbi::default();
    let handle = Handle(1);
    abi.entries.insert(
        handle,
        Entry {
            tree: RustFilesystem::new(Filesystem::new(Limits::default(), 100)),
        },
    );
    write(&mut abi, handle, "file", "bytes");
    let mut body = path("file");
    for value in [0o751_u32, 123, 456] {
        body.extend(value.to_le_bytes());
    }
    body.extend(789_u64.to_le_bytes());
    body.extend(123_456_789_u32.to_le_bytes());
    body.extend(900_u64.to_le_bytes());
    body.extend(987_654_321_u32.to_le_bytes());
    assert_eq!(invoke(&mut abi, handle, 14, &body), Ok(0));
    let before = abi.entries[&handle]
        .tree
        .with_filesystem(|fs| fs.inode(fs.lookup("file").unwrap()).unwrap().clone());
    assert_eq!((before.mode, before.uid, before.gid), (0o751, 123, 456));
    assert_eq!((before.atime, before.atime_nanoseconds), (789, 123_456_789));
    assert_eq!((before.mtime, before.mtime_nanoseconds), (900, 987_654_321));

    for end in 0..body.len() {
        assert_eq!(
            invoke(&mut abi, handle, 14, &body[..end]),
            Err(Error::Packet)
        );
    }
    let end = body.len();
    body[end - 4..].copy_from_slice(&1_000_000_000_u32.to_le_bytes());
    assert_eq!(
        invoke(&mut abi, handle, 14, &body),
        Err(Error::Filesystem(FilesystemError::InvalidPath))
    );
    abi.entries[&handle].tree.with_filesystem(|fs| {
        assert_eq!(fs.inode(fs.lookup("file").unwrap()).unwrap(), &before);
    });
}
