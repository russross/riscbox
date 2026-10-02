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
