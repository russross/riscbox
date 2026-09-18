use riscbox::browser_storage::{HttpBlockStore, HttpFile, StorageError};

#[test]
fn split_image_requests_relative_blocks_and_reads_across_them() {
    let mut store = HttpBlockStore::from_manifest(
        "https://host/vm/drive/blk.txt",
        "{ block_size: 1, n_block: 3, prefetch: [1], }",
        2048,
    )
    .expect("valid manifest");
    let prefetch = store.next_request().expect("prefetch request");
    assert_eq!(prefetch.url, "https://host/vm/drive/blk000000001.bin");
    store
        .complete(prefetch.id, vec![0x22; 1024])
        .expect("block response");

    let mut bytes = [0; 1024];
    assert_eq!(
        store.read_sectors(1, &mut bytes),
        Err(StorageError::MissingBlock(0))
    );
    let request = store.next_request().expect("missing block request");
    store
        .complete(request.id, vec![0x11; 1024])
        .expect("block response");
    store.read_sectors(1, &mut bytes).expect("cached read");
    assert_eq!(&bytes[..512], &[0x11; 512]);
    assert_eq!(&bytes[512..], &[0x22; 512]);
}

#[test]
fn writes_are_copy_on_write_and_ranges_are_checked() {
    let mut store = HttpBlockStore::from_manifest("disk.json", "{block_size:4,n_block:1}", 4096)
        .expect("valid manifest");
    let mut byte = [0; 512];
    assert_eq!(
        store.write_sectors(2, &[7; 512]),
        Err(StorageError::MissingBlock(0))
    );
    let request = store.next_request().expect("original block request");
    store
        .complete(request.id, vec![3; 4096])
        .expect("block response");
    store.write_sectors(2, &[7; 512]).expect("overlay write");
    store.read_sectors(2, &mut byte).expect("overlay read");
    assert_eq!(byte, [7; 512]);
    assert_eq!(
        store.read_sectors(8, &mut byte),
        Err(StorageError::OutOfRange)
    );
}

#[test]
fn manifest_and_plain_http_file_validation_are_explicit() {
    assert!(matches!(
        HttpBlockStore::from_manifest("disk", "{block_size:3,n_block:1}", 1024),
        Err(StorageError::InvalidManifest("invalid block_size"))
    ));
    let file = HttpFile::new("root.bin".to_owned(), None);
    let mut data = b"plain".to_vec();
    assert_eq!(file.decode(&mut data).expect("plain file"), b"plain");
}
