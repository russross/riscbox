use riscbox::browser_storage::{HttpBlockStore, StorageError};

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
    store.write_sectors(2, &[7; 512]).expect("overlay write");
    assert!(store.next_request().is_none());
    store.reset_requests();
    store.read_sectors(2, &mut byte).expect("overlay read");
    assert_eq!(byte, [7; 512]);
    assert_eq!(
        store.read_sectors(8, &mut byte),
        Err(StorageError::OutOfRange)
    );
}

#[test]
fn sparse_overlays_merge_with_base_across_clusters_and_survive_cache_eviction() {
    let mut store =
        HttpBlockStore::from_manifest("disk", "{block_size:4,n_block:3}", 4096).unwrap();
    store.write_sectors(7, &[9; 1024]).unwrap();
    assert!(store.next_request().is_none());
    let mut dirty = [0; 1024];
    store.read_sectors(7, &mut dirty).unwrap();
    assert_eq!(dirty, [9; 1024]);
    let mut combined = [0; 2048];
    for (block, fill) in [(0, 3), (1, 4)] {
        assert_eq!(
            store.read_sectors(6, &mut combined),
            Err(StorageError::MissingBlock(block))
        );
        let request = store.next_request().unwrap();
        store.complete(request.id, vec![fill; 4096]).unwrap();
    }
    store.read_sectors(6, &mut combined).unwrap();
    assert_eq!(&combined[..512], &[3; 512]);
    assert_eq!(&combined[512..1536], &[9; 1024]);
    assert_eq!(&combined[1536..], &[4; 512]);
    // A late base response cannot overwrite data written while it was pending.
    let mut third = [0; 512];
    assert_eq!(
        store.read_sectors(16, &mut third),
        Err(StorageError::MissingBlock(2))
    );
    let request = store.next_request().unwrap();
    store.write_sectors(16, &[7; 512]).unwrap();
    store.complete(request.id, vec![5; 4096]).unwrap();
    store.read_sectors(16, &mut third).unwrap();
    assert_eq!(third, [7; 512]);
    store.discard_changes();
    store.read_sectors(16, &mut third).unwrap();
    assert_eq!(third, [5; 512]);
}

#[test]
fn duplicate_misses_share_fetches_and_failed_requests_can_retry() {
    let mut store =
        HttpBlockStore::from_manifest("disk", "{block_size:1,n_block:2}", 1024).unwrap();
    for sector in [0, 1] {
        assert_eq!(
            store.read_sectors(sector, &mut [0; 512]),
            Err(StorageError::MissingBlock(0))
        );
    }
    let request = store.next_request().unwrap();
    assert!(store.next_request().is_none());
    store.fail(request.id).unwrap();
    assert_eq!(
        store.read_sectors(0, &mut [0; 512]),
        Err(StorageError::BackingFailed)
    );
    store.clear_errors();
    assert_eq!(
        store.read_sectors(0, &mut [0; 512]),
        Err(StorageError::MissingBlock(0))
    );
    let replacement = store.next_request().unwrap();
    assert_ne!(request.id, replacement.id);
    assert_eq!(
        store.complete(replacement.id, vec![0; 511]),
        Err(StorageError::InvalidResponse)
    );
    assert_eq!(
        store.read_sectors(0, &mut [0; 512]),
        Err(StorageError::BackingFailed)
    );
}

#[test]
fn prefetch_failures_retry_on_demand_but_joined_readers_receive_errors() {
    let manifest = "{block_size:1,n_block:1,prefetch:[0]}";
    for joined in [false, true] {
        let mut store = HttpBlockStore::from_manifest("disk", manifest, 1024).unwrap();
        let request = store.next_request().unwrap();
        if joined {
            assert_eq!(
                store.read_sectors(0, &mut [0; 512]),
                Err(StorageError::MissingBlock(0))
            );
            assert!(store.next_request().is_none());
        }
        store.fail(request.id).unwrap();
        let result = store.read_sectors(0, &mut [0; 512]);
        if joined {
            assert_eq!(result, Err(StorageError::BackingFailed));
        } else {
            assert_eq!(result, Err(StorageError::MissingBlock(0)));
            assert_ne!(store.next_request().unwrap().id, request.id);
        }
    }
}

#[test]
fn manifest_validation_is_explicit() {
    assert!(matches!(
        HttpBlockStore::from_manifest("disk", "{block_size:3,n_block:1}", 1024),
        Err(StorageError::InvalidManifest("invalid block_size"))
    ));
}

#[test]
fn high_sectors_keep_their_full_address() {
    let mut store = HttpBlockStore::from_manifest(
        "https://host/disk/blk.txt",
        "{block_size:64,n_block:65537}",
        65_536,
    )
    .expect("large disk");
    assert_eq!(
        store.read_sectors(1 << 23, &mut [0; 512]),
        Err(StorageError::MissingBlock(65_536))
    );
    assert_eq!(
        store.next_request().expect("high block request").url,
        "https://host/disk/blk000065536.bin"
    );
}

#[test]
fn one_request_can_span_more_blocks_than_the_initial_cache_limit() {
    let mut store = HttpBlockStore::from_manifest(
        "https://host/disk/blk.txt",
        "{block_size:4,n_block:2}",
        4096,
    )
    .expect("two-block disk");
    let mut output = [0; 8192];
    for (block, fill) in [(0, 0x11), (1, 0x22)] {
        assert_eq!(
            store.read_sectors(0, &mut output),
            Err(StorageError::MissingBlock(block))
        );
        let request = store.next_request().expect("missing block request");
        store
            .complete(request.id, vec![fill; 4096])
            .expect("block response");
    }
    store
        .read_sectors(0, &mut output)
        .expect("complete spanning read");
    assert_eq!(&output[..4096], &[0x11; 4096]);
    assert_eq!(&output[4096..], &[0x22; 4096]);
}

#[test]
fn reset_retires_pending_http_requests_without_reusing_their_ids() {
    let mut store = HttpBlockStore::from_manifest(
        "https://host/disk/blk.txt",
        "{block_size:1,n_block:1}",
        1024,
    )
    .expect("disk");
    assert_eq!(
        store.read_sectors(0, &mut [0; 512]),
        Err(StorageError::MissingBlock(0))
    );
    let old = store.next_request().expect("first request");
    store.reset_requests();
    assert_eq!(
        store.complete(old.id, vec![1; 1024]),
        Err(StorageError::UnknownRequest)
    );
    assert_eq!(
        store.read_sectors(0, &mut [0; 512]),
        Err(StorageError::MissingBlock(0))
    );
    let new = store.next_request().expect("replacement request");
    assert_ne!(old.id, new.id);
}
