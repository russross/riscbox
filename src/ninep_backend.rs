//! Rust protocol ownership and explicit source work for `VirtIO` endpoints.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use crate::ninep::{FileBody, Filesystem, FilesystemError, InodeId, LoadStart, LoadTicket};
use crate::ninep_protocol::{NinePSession, Outcome, RequestId, Submission};
use crate::virtio_devices::{
    DeviceError, NinePBackend, NinePCompletion, NinePGeneration, NinePOutcome, NinePRequestId,
};

/// Shared namespace ownership stays separate from device and VM lifetimes.
#[derive(Clone)]
pub struct RustFilesystem {
    filesystem: Rc<RefCell<Filesystem>>,
    loads: Rc<RefCell<VecDeque<LoadTicket>>>,
    attachment: Rc<RefCell<Weak<()>>>,
}

impl RustFilesystem {
    #[must_use]
    pub fn new(filesystem: Filesystem) -> Self {
        Self {
            filesystem: Rc::new(RefCell::new(filesystem)),
            loads: Rc::new(RefCell::new(VecDeque::new())),
            attachment: Rc::new(RefCell::new(Weak::new())),
        }
    }

    /// Host access must finish before CPU execution or source dispatch starts.
    pub fn with_filesystem<T>(&self, operation: impl FnOnce(&mut Filesystem) -> T) -> T {
        operation(&mut self.filesystem.borrow_mut())
    }

    #[must_use]
    pub fn next_load(&self) -> Option<LoadTicket> {
        self.prune_loads();
        self.loads.borrow_mut().pop_front()
    }

    /// Starts or joins namespace source work for a host operation.
    ///
    /// # Errors
    /// Returns the namespace load validation error.
    pub fn begin_load(&self, inode: InodeId) -> Result<LoadTicket, FilesystemError> {
        let start = self.with_filesystem(|fs| fs.begin_load(inode))?;
        if let LoadStart::Started(ticket) = start {
            self.loads.borrow_mut().push_back(ticket);
        }
        Ok(start.ticket())
    }

    #[must_use]
    pub fn same_tree(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.filesystem, &other.filesystem)
    }

    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.attachment.borrow().upgrade().is_some()
    }

    /// Claims a tree for one VM; several endpoints share this one claim.
    ///
    /// # Errors
    /// Returns an error while another VM retains the attachment.
    pub fn attach(&self) -> Result<FilesystemAttachment, FilesystemError> {
        if self.is_attached() {
            return Err(FilesystemError::AlreadyExists);
        }
        let token = Rc::new(());
        *self.attachment.borrow_mut() = Rc::downgrade(&token);
        Ok(FilesystemAttachment { _token: token })
    }

    fn prune_loads(&self) {
        // A host write or namespace reset may supersede source work before
        // the runtime gets back to JavaScript. Do not dispatch those tickets.
        let filesystem = self.filesystem.borrow();
        self.loads.borrow_mut().retain(|ticket| {
            ticket.generation == filesystem.generation()
                && matches!(filesystem.file_body(ticket.inode), Ok(FileBody::Loading { load, source, size })
                    if *load == ticket.id && *source == ticket.source && *size == ticket.size)
        });
    }
}

/// Dropping the VM's claim permits a later VM to attach the retained tree.
pub struct FilesystemAttachment {
    _token: Rc<()>,
}

pub struct RustNineP {
    tree: RustFilesystem,
    session: NinePSession,
    generation: NinePGeneration,
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
            generation: NinePGeneration(1),
            failed: false,
        })
    }
}

impl NinePBackend for RustNineP {
    fn submit(
        &mut self,
        request_id: NinePRequestId,
        request: Vec<u8>,
        reply_capacity: u32,
    ) -> Option<NinePOutcome> {
        if self.failed {
            return Some(NinePOutcome::EndpointFailure);
        }
        let result = self.session.submit(
            &mut self.tree.filesystem.borrow_mut(),
            RequestId(u64::from(request_id.0)),
            &request,
            reply_capacity as usize,
        );
        match result {
            Ok(Submission::Immediate(reply)) => Some(NinePOutcome::Reply(reply)),
            Ok(Submission::Pending(LoadStart::Started(ticket))) => {
                self.tree.loads.borrow_mut().push_back(ticket);
                None
            }
            Ok(Submission::Pending(LoadStart::Joined(_))) => None,
            Err(_) => {
                self.failed = true;
                Some(NinePOutcome::EndpointFailure)
            }
        }
    }

    fn next_completion(&mut self) -> Result<Option<NinePCompletion>, DeviceError> {
        self.session
            .poll(&mut self.tree.filesystem.borrow_mut())
            .map_err(|_| DeviceError::Backend)?;
        self.session
            .next_completion()
            .map(|completion| {
                Ok(NinePCompletion {
                    generation: self.generation,
                    request: NinePRequestId(
                        u32::try_from(completion.request.0).map_err(|_| DeviceError::Backend)?,
                    ),
                    outcome: match completion.outcome {
                        Outcome::Reply(bytes) => NinePOutcome::Reply(bytes),
                        Outcome::Suppressed => NinePOutcome::Suppressed,
                    },
                })
            })
            .transpose()
    }

    fn reset(&mut self, generation: NinePGeneration) {
        // Source loads belong to the namespace, so reset keeps their tickets.
        // The session discards completions for queues the device relinquished.
        self.failed = self
            .session
            .reset(&mut self.tree.filesystem.borrow_mut())
            .is_err();
        self.generation = generation;
    }

    fn has_transport_action(&self) -> bool {
        self.tree.prune_loads();
        !self.tree.loads.borrow().is_empty()
    }
}

impl Drop for RustNineP {
    fn drop(&mut self) {
        // Endpoint teardown releases pins and locks while retaining file data.
        let result = self.session.close(&mut self.tree.filesystem.borrow_mut());
        debug_assert!(result.is_ok(), "bound Rust 9p session failed to close");
    }
}
