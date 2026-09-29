//! Standalone raw-WASM namespace probe, with no WASI or JavaScript imports.

#[path = "../../src/ninep.rs"]
pub mod ninep;

use ninep::{ChangeKind, FileRead, Filesystem, Limits, LoadStart, SeedMetadata, SourceId};

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
