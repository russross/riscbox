use riscbox::host_block::{
    HostBlockGeneration, HostBlockKind, HostBlockOutcome, HostBlockProviderId, HostBlockRequestId,
    HostBlockStore,
};
use riscbox::virtio_devices::{BlockBackend, BlockRequestStatus, DeviceError};

#[test]
fn host_requests_preserve_sector_data_and_retire_old_generations() {
    let mut store = HostBlockStore::new(HostBlockProviderId(3), 16);
    let mut read = [0; 512];
    assert_eq!(
        store.read_request(9, &mut read),
        Ok(BlockRequestStatus::Pending)
    );
    let request = store.next_request().expect("read request");
    assert_eq!(
        (
            request.provider,
            request.generation,
            request.kind,
            request.sector,
            request.length
        ),
        (
            HostBlockProviderId(3),
            HostBlockGeneration(1),
            HostBlockKind::Read,
            9,
            512
        )
    );
    assert!(store.next_request().is_none());
    store
        .complete(
            HostBlockGeneration(1),
            request.id,
            HostBlockOutcome::Success(vec![0x5a; 512]),
        )
        .expect("reply");
    assert_eq!(
        store.read_request(9, &mut read),
        Ok(BlockRequestStatus::Complete)
    );
    assert_eq!(read, [0x5a; 512]);

    let write = [0x35; 512];
    assert_eq!(
        store.write_request(2, &write),
        Ok(BlockRequestStatus::Pending)
    );
    let request = store.next_request().expect("write request");
    assert_eq!(
        (request.kind, request.sector, request.data),
        (HostBlockKind::Write, 2, write.to_vec())
    );
    store.reset();
    assert_eq!(store.generation(), HostBlockGeneration(2));
    assert!(
        !store
            .complete(
                HostBlockGeneration(1),
                request.id,
                HostBlockOutcome::Success(Vec::new())
            )
            .expect("retired reply")
    );
    assert!(store.next_request().is_none());
}

#[test]
fn malformed_and_failed_responses_become_guest_io_errors() {
    let mut store = HostBlockStore::new(HostBlockProviderId(1), 8);
    let mut read = [0; 512];
    store.read_request(0, &mut read).expect("start read");
    let request = store.next_request().expect("request");
    assert_eq!(
        store.complete(
            HostBlockGeneration(1),
            HostBlockRequestId(request.id.0 + 1),
            HostBlockOutcome::Success(vec![1; 512])
        ),
        Err(DeviceError::InvalidRequest)
    );
    store
        .complete(
            HostBlockGeneration(1),
            request.id,
            HostBlockOutcome::Success(vec![1; 511]),
        )
        .expect("malformed result");
    assert_eq!(store.read_request(0, &mut read), Err(DeviceError::Backend));
    store.write_request(1, &[2; 512]).expect("start write");
    let write = store.next_request().expect("write request");
    store
        .complete(HostBlockGeneration(1), write.id, HostBlockOutcome::IoError)
        .expect("provider failure");
    assert_eq!(store.write_request(1, &[2; 512]), Err(DeviceError::Backend));
}
