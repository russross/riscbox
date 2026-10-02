//! Rust protocol ownership and synchronous replies for `VirtIO` endpoints.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ninep::{Filesystem, FilesystemError};
use crate::ninep_protocol::NinePSession;
use crate::virtio_devices::{NinePBackend, NinePGeneration, NinePOutcome, NinePRequestId};

/// Shared namespace ownership stays separate from device and VM lifetimes.
#[derive(Clone)]
pub struct RustFilesystem {
    filesystem: Rc<RefCell<Filesystem>>,
}

impl RustFilesystem {
    #[must_use]
    pub fn new(filesystem: Filesystem) -> Self {
        Self {
            filesystem: Rc::new(RefCell::new(filesystem)),
        }
    }

    /// Host access must finish before CPU execution starts.
    pub fn with_filesystem<T>(&self, operation: impl FnOnce(&mut Filesystem) -> T) -> T {
        operation(&mut self.filesystem.borrow_mut())
    }
}

pub struct RustNineP {
    tree: RustFilesystem,
    session: NinePSession,
    failed: bool,
}

impl RustNineP {
    /// Creates an independent protocol endpoint over shared namespace state.
    ///
    /// # Errors
    /// Returns an error if the namespace cannot allocate a session identity.
    pub fn new(tree: RustFilesystem) -> Result<Self, FilesystemError> {
        let session = tree.with_filesystem(NinePSession::new)?;
        Ok(Self {
            tree,
            session,
            failed: false,
        })
    }
}

impl NinePBackend for RustNineP {
    fn submit(
        &mut self,
        _request_id: NinePRequestId,
        request: Vec<u8>,
        reply_capacity: u32,
    ) -> Option<NinePOutcome> {
        if self.failed {
            return Some(NinePOutcome::EndpointFailure);
        }
        let result = self.session.submit(
            &mut self.tree.filesystem.borrow_mut(),
            &request,
            reply_capacity as usize,
        );
        if let Ok(reply) = result {
            Some(NinePOutcome::Reply(reply))
        } else {
            self.failed = true;
            Some(NinePOutcome::EndpointFailure)
        }
    }

    fn reset(&mut self, _generation: NinePGeneration) {
        // Reset releases guest fids and locks while retaining namespace bytes.
        self.failed = self
            .session
            .reset(&mut self.tree.filesystem.borrow_mut())
            .is_err();
    }
}

impl Drop for RustNineP {
    fn drop(&mut self) {
        // Endpoint teardown releases pins and locks while retaining file data.
        let result = self.session.close(&mut self.tree.filesystem.borrow_mut());
        debug_assert!(result.is_ok(), "bound Rust 9p session failed to close");
    }
}
