//! A synchronous 9P2000.L endpoint over a resident in-memory namespace.

use std::collections::BTreeMap;
use std::mem::take;
use std::str::from_utf8;

use crate::ninep::{
    AttributeUpdate, ByteRangeLock, ChangeSource, FileTime, Filesystem, FilesystemError,
    FilesystemIdentity, Inode, InodeId, InodeKind, LockKind, SharedFilesystem, TimeUpdate,
};

/// A device endpoint owns one session over a VM-owned shared namespace.
pub struct NinePEndpoint {
    tree: SharedFilesystem,
    session: NinePSession,
    failed: bool,
}

impl NinePEndpoint {
    /// Creates an independent protocol endpoint over shared namespace state.
    /// # Errors
    /// Returns an error if the namespace cannot allocate a session identity.
    pub fn new(tree: SharedFilesystem) -> Result<Self, FilesystemError> {
        let session = tree.with_filesystem(NinePSession::new)?;
        Ok(Self {
            tree,
            session,
            failed: false,
        })
    }

    /// Completes one resident request before returning to the device.
    /// # Errors
    /// Reports protocol failure and rejects further requests until reset.
    pub fn submit(
        &mut self,
        request: &[u8],
        reply_capacity: usize,
    ) -> Result<Vec<u8>, ProtocolError> {
        if self.failed {
            return Err(ProtocolError::Closed);
        }
        let result = self
            .tree
            .with_filesystem(|fs| self.session.submit(fs, request, reply_capacity));
        self.failed = result.is_err();
        result
    }

    /// Reset releases guest fids and locks while retaining namespace bytes.
    pub fn reset(&mut self) {
        self.failed = self
            .tree
            .with_filesystem(|fs| self.session.reset(fs))
            .is_err();
    }
}

impl Drop for NinePEndpoint {
    fn drop(&mut self) {
        // Endpoint teardown releases pins and locks while retaining file data.
        let result = self.tree.with_filesystem(|fs| self.session.close(fs));
        debug_assert!(result.is_ok(), "bound 9p session failed to close");
    }
}

const MAX_MESSAGE_SIZE: usize = 64 * 1024;
const MAX_FIDS: usize = 65_536;
const NOTAG: u16 = u16::MAX;
const NOFID: u32 = u32::MAX;
const EPERM: u32 = 1;
const EIO: u32 = 5;
const EBADF: u32 = 9;
const EBUSY: u32 = 16;
const EEXIST: u32 = 17;
const ENOTDIR: u32 = 20;
const EISDIR: u32 = 21;
const EINVAL: u32 = 22;
const ENOSPC: u32 = 28;
const ELOOP: u32 = 40;
const EPROTO: u32 = 71;
const EOPNOTSUPP: u32 = 95;
const ESTALE: u32 = 116;

/// These are host/transport errors that cannot safely be encoded as a reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    Malformed,
    ReplyTooSmall,
    WrongFilesystem,
    Closed,
    Invariant,
}

#[derive(Clone, Copy, Debug)]
enum Access {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Copy, Debug)]
struct OpenFlags {
    access: Access,
    raw: u32,
}

impl OpenFlags {
    fn parse(flags: u32) -> Result<Self, u32> {
        // The .L flags are Linux's fixed wire values, not host-platform libc bits.
        // O_PATH and O_TMPFILE need distinct inode semantics and are unsupported.
        if flags & !0x001f_ffff != 0 {
            return Err(EOPNOTSUPP);
        }
        let access = match flags & 3 {
            0 => Access::Read,
            1 => Access::Write,
            2 => Access::ReadWrite,
            _ => return Err(EINVAL),
        };
        let result = Self { access, raw: flags };
        if result.truncate() && !result.writable() {
            return Err(EINVAL);
        }
        Ok(result)
    }
    const fn readable(self) -> bool {
        matches!(self.access, Access::Read | Access::ReadWrite)
    }
    const fn writable(self) -> bool {
        matches!(self.access, Access::Write | Access::ReadWrite)
    }
    const fn append(self) -> bool {
        self.raw & 0x400 != 0
    }
    const fn truncate(self) -> bool {
        self.raw & 0x200 != 0
    }
    const fn directory(self) -> bool {
        self.raw & 0x10000 != 0
    }
    const fn no_atime(self) -> bool {
        self.raw & 0x40000 != 0
    }
}

#[derive(Clone, Copy, Debug)]
struct Fid {
    inode: InodeId,
    uid: u32,
    open: Option<OpenFlags>,
}

// File operations resolve their fid to a stable inode before changing data.
// Write bytes are borrowed until the synchronous reply is constructed.
enum IoOperation<'a> {
    Read {
        inode: InodeId,
        offset: u64,
        count: usize,
        no_atime: bool,
    },
    Write {
        inode: InodeId,
        offset: u64,
        data: &'a [u8],
        append: bool,
    },
    Attributes {
        inode: InodeId,
        update: AttributeUpdate,
    },
}

/// A session is bound to one namespace identity; its protocol state is independent.
pub struct NinePSession {
    identity: FilesystemIdentity,
    generation: u64,
    session: u64,
    fids: BTreeMap<u32, Fid>,
    msize: usize,
    negotiated: bool,
    closed: bool,
}

impl NinePSession {
    /// # Errors
    /// Reports exhaustion of the namespace's session IDs.
    pub fn new(filesystem: &mut Filesystem) -> Result<Self, FilesystemError> {
        Ok(Self {
            identity: filesystem.identity(),
            generation: filesystem.generation(),
            session: filesystem.allocate_session()?,
            fids: BTreeMap::new(),
            msize: MAX_MESSAGE_SIZE,
            negotiated: false,
            closed: false,
        })
    }

    fn errno(error: FilesystemError) -> u32 {
        match error {
            FilesystemError::NotFound => 2,
            FilesystemError::NotDirectory => ENOTDIR,
            FilesystemError::IsDirectory => EISDIR,
            FilesystemError::AlreadyExists => EEXIST,
            FilesystemError::NotEmpty => 39,
            FilesystemError::FileTooLarge => 27,
            FilesystemError::NoSpace => ENOSPC,
            FilesystemError::InvalidPath => EINVAL,
            FilesystemError::NameTooLong => 36,
        }
    }

    fn error(tag: u16, errno: u32, capacity: usize) -> Result<Vec<u8>, ProtocolError> {
        let mut reply = Writer::new(7, tag, capacity).map_err(|_| ProtocolError::ReplyTooSmall)?;
        reply.u32(errno).map_err(|_| ProtocolError::ReplyTooSmall)?;
        Ok(reply.finish())
    }

    fn check_identity(&self, fs: &Filesystem) -> Result<(), ProtocolError> {
        if self.identity != fs.identity() {
            return Err(ProtocolError::WrongFilesystem);
        }
        if self.closed {
            return Err(ProtocolError::Closed);
        }
        Ok(())
    }

    // Namespace replacement invalidates old references without releasing them
    // into the replacement inode arena. Live descriptors receive tagged errors.
    fn sync_generation(&mut self, fs: &Filesystem) -> Result<(), ProtocolError> {
        self.check_identity(fs)?;
        if self.generation != fs.generation() {
            self.fids.clear();
            self.generation = fs.generation();
        }
        Ok(())
    }

    fn release_fids(&mut self, fs: &mut Filesystem) -> Result<(), ProtocolError> {
        for (_, fid) in take(&mut self.fids) {
            fs.release_fid(fid.inode)
                .map_err(|_| ProtocolError::Invariant)?;
        }
        fs.release_session_locks(self.session);
        Ok(())
    }

    /// Retires device state without publishing to queues that the driver reset.
    /// # Errors
    /// Rejects a different filesystem or inconsistent retained references.
    pub fn reset(&mut self, fs: &mut Filesystem) -> Result<(), ProtocolError> {
        self.check_identity(fs)?;
        if self.generation == fs.generation() {
            self.release_fids(fs)?;
        } else {
            self.fids.clear();
        }
        self.generation = fs.generation();
        self.msize = MAX_MESSAGE_SIZE;
        self.negotiated = false;
        Ok(())
    }

    /// # Errors
    /// Rejects a different filesystem or inconsistent retained references.
    pub fn close(&mut self, fs: &mut Filesystem) -> Result<(), ProtocolError> {
        if self.closed {
            return if self.identity == fs.identity() {
                Ok(())
            } else {
                Err(ProtocolError::WrongFilesystem)
            };
        }
        self.reset(fs)?;
        self.closed = true;
        Ok(())
    }

    /// Validates and executes one request within this activation.
    /// # Errors
    /// Reports invalid envelopes without tags, wrong
    /// namespaces, closed endpoints, or a reply buffer too small for an error.
    pub fn submit(
        &mut self,
        fs: &mut Filesystem,
        bytes: &[u8],
        capacity: usize,
    ) -> Result<Vec<u8>, ProtocolError> {
        self.sync_generation(fs)?;
        let mut header = Reader::new(bytes);
        let declared = usize::try_from(header.u32().map_err(|_| ProtocolError::Malformed)?)
            .map_err(|_| ProtocolError::Malformed)?;
        let kind = header.u8().map_err(|_| ProtocolError::Malformed)?;
        let tag = header.u16().map_err(|_| ProtocolError::Malformed)?;
        let capacity = capacity.min(if kind == 100 {
            MAX_MESSAGE_SIZE
        } else {
            self.msize
        });
        if declared != bytes.len()
            || declared
                > if kind == 100 {
                    MAX_MESSAGE_SIZE
                } else {
                    self.msize
                }
        {
            if kind == 108 {
                return Err(ProtocolError::Malformed);
            }
            return Self::error(tag, EPROTO, capacity);
        }
        let request = match Request::parse(kind, &bytes[7..]) {
            Ok(request) => request,
            Err(errno) => {
                if kind == 108 {
                    return Err(ProtocolError::Malformed);
                }
                return Self::error(tag, errno, capacity);
            }
        };
        if (tag == NOTAG && kind != 100) || (!self.negotiated && kind != 100 && kind != 108) {
            if kind == 108 {
                return Err(ProtocolError::Malformed);
            }
            return Self::error(tag, EPROTO, capacity);
        }
        if capacity < request.minimum_reply() {
            return Self::error(tag, ENOSPC, capacity);
        }
        let mut reply = Writer::new(kind.wrapping_add(1), tag, capacity)
            .map_err(|_| ProtocolError::ReplyTooSmall)?;
        if let Request::Flush = request {
            return Ok(reply.finish());
        }
        let source = fs.mutation_source();
        fs.set_mutation_source(ChangeSource::Guest);
        let result = self.execute(fs, request, tag, capacity, &mut reply);
        fs.set_mutation_source(source);
        match result {
            Ok(()) => Ok(reply.finish()),
            Err(errno) => Self::error(tag, errno, capacity),
        }
    }

    fn fid(&self, number: u32) -> Result<Fid, u32> {
        self.fids.get(&number).copied().ok_or(EBADF)
    }
    fn directory(&self, fs: &Filesystem, number: u32) -> Result<Fid, u32> {
        let fid = self.fid(number)?;
        if !matches!(
            fs.inode(fid.inode).ok_or(ESTALE)?.kind,
            InodeKind::Directory { .. }
        ) {
            return Err(ENOTDIR);
        }
        Ok(fid)
    }
    fn opened(&self, number: u32) -> Result<(Fid, OpenFlags), u32> {
        let fid = self.fid(number)?;
        Ok((fid, fid.open.ok_or(EBADF)?))
    }
    fn check_new_fid(&self, number: u32) -> Result<(), u32> {
        if number == NOFID {
            return Err(EINVAL);
        }
        if self.fids.contains_key(&number) {
            return Err(EEXIST);
        }
        if self.fids.len() >= MAX_FIDS {
            return Err(ENOSPC);
        }
        Ok(())
    }
    fn install_fid(&mut self, fs: &mut Filesystem, number: u32, fid: Fid) -> Result<(), u32> {
        fs.retain_fid(fid.inode).map_err(Self::errno)?;
        if let Some(old) = self.fids.insert(number, fid) {
            fs.release_fid(old.inode).map_err(Self::errno)?;
        }
        Ok(())
    }

    fn execute(
        &mut self,
        fs: &mut Filesystem,
        request: Request<'_>,
        tag: u16,
        capacity: usize,
        reply: &mut Writer,
    ) -> Result<(), u32> {
        match request {
            Request::Version { size, version } => self.version(fs, tag, size, version, reply)?,
            Request::Attach { fid, afid, uid } => {
                self.check_new_fid(fid)?;
                if afid != NOFID {
                    return Err(EOPNOTSUPP);
                }
                let inode = fs.inode(fs.root()).ok_or(ESTALE)?;
                reply.qid(inode)?;
                self.install_fid(
                    fs,
                    fid,
                    Fid {
                        inode: inode.id,
                        uid: if uid == NOFID { inode.uid } else { uid },
                        open: None,
                    },
                )?;
            }
            Request::Walk {
                fid,
                new_fid,
                names,
            } => self.walk(fs, fid, new_fid, &names, reply)?,
            Request::Open { fid, flags } => self.open(fs, fid, flags, reply)?,
            Request::Create { fid, file } => self.create(fs, fid, file, reply)?,
            Request::Read { fid, offset, count } => {
                let (fid, flags) = self.opened(fid)?;
                if !flags.readable() {
                    return Err(EBADF);
                }
                return Self::finish_io(
                    fs,
                    &IoOperation::Read {
                        inode: fid.inode,
                        offset,
                        count: usize::try_from(count)
                            .map_err(|_| EINVAL)?
                            .min(capacity - 11),
                        no_atime: flags.no_atime(),
                    },
                    reply,
                );
            }
            Request::Write { fid, offset, data } => {
                let (fid, flags) = self.opened(fid)?;
                if !flags.writable() {
                    return Err(EBADF);
                }
                return Self::finish_io(
                    fs,
                    &IoOperation::Write {
                        inode: fid.inode,
                        offset,
                        data,
                        append: flags.append(),
                    },
                    reply,
                );
            }
            Request::Setattr { fid, attributes } => {
                let inode = self.fid(fid)?.inode;
                let update = Self::attributes(attributes)?;
                return Self::finish_io(fs, &IoOperation::Attributes { inode, update }, reply);
            }
            Request::Clunk { fid } => {
                let inode = self.fid(fid)?.inode;
                fs.release_fid(inode).map_err(Self::errno)?;
                self.fids.remove(&fid);
            }
            other => self.filesystem_operation(fs, &other, reply)?,
        }
        Ok(())
    }

    fn version(
        &mut self,
        fs: &mut Filesystem,
        tag: u16,
        size: u32,
        version: &str,
        reply: &mut Writer,
    ) -> Result<(), u32> {
        if tag != NOTAG {
            return Err(EPROTO);
        }
        let size = usize::try_from(size)
            .map_err(|_| EINVAL)?
            .min(MAX_MESSAGE_SIZE);
        if size < 256 {
            return Err(EINVAL);
        }
        reply.u32(u32::try_from(size).expect("bounded size"))?;
        reply.string(if version == "9P2000.L" {
            version
        } else {
            "unknown"
        })?;
        self.release_fids(fs).map_err(|_| EIO)?;
        self.msize = size;
        self.negotiated = version == "9P2000.L";
        Ok(())
    }

    fn walk(
        &mut self,
        fs: &mut Filesystem,
        number: u32,
        new_fid: u32,
        names: &[&str],
        reply: &mut Writer,
    ) -> Result<(), u32> {
        let fid = self.fid(number)?;
        if fid.open.is_some() {
            return Err(EBADF);
        }
        if new_fid != number {
            self.check_new_fid(new_fid)?;
        }
        let mut current = fid.inode;
        let mut qids = Vec::new();
        for name in names {
            let result = match fs.inode(current).ok_or(ESTALE)?.kind {
                InodeKind::Directory { .. } => match *name {
                    "." => Ok(current),
                    ".." if current == fs.root() => Ok(current),
                    ".." => fs.parent_inode(current).ok_or(2_u32),
                    name => fs.child(current, name).map_err(Self::errno),
                },
                _ => Err(ENOTDIR),
            };
            match result {
                Ok(id) => {
                    current = id;
                    qids.push(id);
                }
                Err(errno) if qids.is_empty() => return Err(errno),
                Err(_) => break,
            }
        }
        reply.u16(u16::try_from(qids.len()).expect("bounded walk count"))?;
        for id in &qids {
            reply.qid(fs.inode(*id).ok_or(ESTALE)?)?;
        }
        if qids.len() == names.len() {
            self.install_fid(
                fs,
                new_fid,
                Fid {
                    inode: current,
                    ..fid
                },
            )?;
        }
        Ok(())
    }

    fn open(
        &mut self,
        fs: &mut Filesystem,
        number: u32,
        raw: u32,
        reply: &mut Writer,
    ) -> Result<(), u32> {
        let fid = self.fid(number)?;
        if fid.open.is_some() {
            return Err(EBUSY);
        }
        let flags = OpenFlags::parse(raw)?;
        let inode = fs.inode(fid.inode).ok_or(ESTALE)?;
        match inode.kind {
            InodeKind::Directory { .. } if flags.writable() || flags.truncate() => {
                return Err(EISDIR);
            }
            InodeKind::Symlink(_) => return Err(ELOOP),
            InodeKind::File(_) if flags.directory() => return Err(ENOTDIR),
            _ => {}
        }
        if flags.truncate() {
            fs.update_attributes(
                fid.inode,
                AttributeUpdate {
                    size: Some(0),
                    ..AttributeUpdate::default()
                },
            )
            .map_err(Self::errno)?;
        }
        reply.qid(fs.inode(fid.inode).ok_or(ESTALE)?)?;
        reply.u32(u32::try_from(self.msize - 24).expect("bounded iounit"))?;
        self.fids.get_mut(&number).expect("validated fid").open = Some(flags);
        Ok(())
    }

    fn create(
        &mut self,
        fs: &mut Filesystem,
        number: u32,
        file: Creation<'_>,
        reply: &mut Writer,
    ) -> Result<(), u32> {
        let fid = self.directory(fs, number)?;
        if fid.open.is_some() {
            return Err(EBADF);
        }
        let flags = OpenFlags::parse(file.flags)?;
        if flags.directory() {
            return Err(ENOTDIR);
        }
        // Tlcreate has Linux O_CREAT semantics: an existing regular file can
        // be opened, with O_EXCL and O_TRUNC selecting the race behavior.
        let inode = match fs.child(fid.inode, file.name) {
            Ok(id) => {
                if file.flags & 0x80 != 0 {
                    return Err(EEXIST);
                }
                match fs.inode(id).ok_or(ESTALE)?.kind {
                    InodeKind::Directory { .. } => return Err(EISDIR),
                    InodeKind::Symlink(_) => return Err(ELOOP),
                    InodeKind::File(_) => {}
                }
                if flags.truncate() {
                    fs.update_attributes(
                        id,
                        AttributeUpdate {
                            size: Some(0),
                            ..AttributeUpdate::default()
                        },
                    )
                    .map_err(Self::errno)?;
                }
                id
            }
            Err(FilesystemError::NotFound) => fs
                .create_file_at(fid.inode, file.name, file.mode, fid.uid, file.gid)
                .map_err(Self::errno)?,
            Err(error) => return Err(Self::errno(error)),
        };
        self.install_fid(
            fs,
            number,
            Fid {
                inode,
                uid: fid.uid,
                open: Some(flags),
            },
        )?;
        reply.qid(fs.inode(inode).ok_or(ESTALE)?)?;
        reply.u32(u32::try_from(self.msize - 24).expect("bounded iounit"))
    }

    fn attributes(raw: Attributes) -> Result<AttributeUpdate, u32> {
        if raw.mask & !0x1ff != 0
            || (raw.mask & 0x80 != 0 && raw.mask & 0x10 == 0)
            || (raw.mask & 0x100 != 0 && raw.mask & 0x20 == 0)
        {
            return Err(EINVAL);
        }
        let time = |bit, explicit, seconds, ns| -> Result<Option<TimeUpdate>, u32> {
            if raw.mask & bit == 0 {
                return Ok(None);
            }
            if raw.mask & explicit == 0 {
                return Ok(Some(TimeUpdate::Current));
            }
            FileTime::new(seconds, ns)
                .map(|time| Some(TimeUpdate::Explicit(time)))
                .map_err(Self::errno)
        };
        Ok(AttributeUpdate {
            mode: (raw.mask & 1 != 0).then_some(raw.mode),
            uid: (raw.mask & 2 != 0).then_some(raw.uid),
            gid: (raw.mask & 4 != 0).then_some(raw.gid),
            size: (raw.mask & 8 != 0).then_some(raw.size),
            atime: time(0x10, 0x80, raw.atime, raw.atime_ns)?,
            mtime: time(0x20, 0x100, raw.mtime, raw.mtime_ns)?,
            change_ctime: raw.mask & 0x40 != 0,
        })
    }

    fn finish_io(
        fs: &mut Filesystem,
        operation: &IoOperation<'_>,
        reply: &mut Writer,
    ) -> Result<(), u32> {
        match operation {
            IoOperation::Read {
                inode,
                offset,
                count,
                no_atime,
            } => {
                let bytes = fs
                    .read_inode(*inode, *offset, *count, !no_atime)
                    .map_err(Self::errno)?;
                reply.u32(u32::try_from(bytes.len()).expect("bounded read"))?;
                reply.append(bytes)
            }
            IoOperation::Write {
                inode,
                offset,
                data,
                append,
            } => {
                fs.write_inode(*inode, *offset, data, *append)
                    .map_err(Self::errno)?;
                reply.u32(u32::try_from(data.len()).expect("bounded write"))
            }
            IoOperation::Attributes { inode, update } => {
                fs.update_attributes(*inode, *update).map_err(Self::errno)
            }
        }
    }

    fn filesystem_operation(
        &self,
        fs: &mut Filesystem,
        request: &Request<'_>,
        reply: &mut Writer,
    ) -> Result<(), u32> {
        match request {
            Request::Statfs { fid } => {
                self.fid(*fid)?;
                let usage = fs.space_usage();
                reply.u32(0x0102_1997)?;
                reply.u32(4096)?;
                reply.u64(u64::try_from(usage.maximum_bytes / 4096).expect("bounded quota"))?;
                let free = u64::try_from((usage.maximum_bytes - usage.used_bytes) / 4096)
                    .expect("bounded quota");
                reply.u64(free)?;
                reply.u64(free)?;
                reply.u64(u64::try_from(usage.maximum_inodes).expect("bounded inode quota"))?;
                reply.u64(
                    u64::try_from(usage.maximum_inodes.saturating_sub(usage.used_inodes))
                        .expect("bounded inode quota"),
                )?;
                reply.u64(fs.root().0)?;
                reply.u32(255)
            }
            Request::Getattr { fid, mask } => {
                Self::getattr(fs, self.fid(*fid)?.inode, *mask, reply)
            }
            Request::Readdir { fid, offset, count } => {
                self.readdir(fs, *fid, *offset, *count, reply)
            }
            Request::Fsync { fid, data_only } => {
                self.opened(*fid)?;
                if *data_only > 1 {
                    return Err(EINVAL);
                }
                Ok(())
            }
            Request::Readlink { fid } => {
                match &fs.inode(self.fid(*fid)?.inode).ok_or(ESTALE)?.kind {
                    InodeKind::Symlink(target) => reply.string(target),
                    _ => Err(EINVAL),
                }
            }
            Request::Symlink {
                fid,
                name,
                target,
                gid,
            } => {
                let directory = self.directory(fs, *fid)?;
                let id = fs
                    .symlink_at(directory.inode, name, target, directory.uid, *gid)
                    .map_err(Self::errno)?;
                reply.qid(fs.inode(id).ok_or(ESTALE)?)
            }
            Request::Mkdir {
                fid,
                name,
                mode,
                gid,
            } => {
                let directory = self.directory(fs, *fid)?;
                let id = fs
                    .mkdir_at(directory.inode, name, *mode, directory.uid, *gid)
                    .map_err(Self::errno)?;
                reply.qid(fs.inode(id).ok_or(ESTALE)?)
            }
            Request::Link {
                directory,
                fid,
                name,
            } => {
                let directory = self.directory(fs, *directory)?;
                let inode = self.fid(*fid)?.inode;
                if matches!(
                    fs.inode(inode).ok_or(ESTALE)?.kind,
                    InodeKind::Directory { .. }
                ) {
                    return Err(EPERM);
                }
                fs.link_at(directory.inode, name, inode)
                    .map_err(Self::errno)
            }
            Request::Rename {
                old_directory,
                old_name,
                new_directory,
                new_name,
            } => {
                let old = self.directory(fs, *old_directory)?.inode;
                let new = self.directory(fs, *new_directory)?.inode;
                fs.rename_at(old, old_name, new, new_name)
                    .map_err(Self::errno)
            }
            Request::Unlink {
                directory,
                name,
                flags,
            } => {
                if flags & !0x200 != 0 {
                    return Err(EINVAL);
                }
                let parent = self.directory(fs, *directory)?.inode;
                let inode = fs.child(parent, name).map_err(Self::errno)?;
                let is_directory = matches!(
                    fs.inode(inode).ok_or(ESTALE)?.kind,
                    InodeKind::Directory { .. }
                );
                if (flags & 0x200 != 0) != is_directory {
                    return Err(if is_directory { EISDIR } else { ENOTDIR });
                }
                fs.remove_at(parent, name).map_err(Self::errno)
            }
            Request::Lock(lock) => self.lock(fs, *lock, reply),
            Request::Getlock(lock) => self.getlock(fs, *lock, reply),
            _ => Err(EOPNOTSUPP),
        }
    }

    // .L validity bits describe only attributes this server actually supplies.
    // Birth time, inode generation, and data-version extensions stay unclaimed.
    fn getattr(fs: &Filesystem, id: InodeId, mask: u64, reply: &mut Writer) -> Result<(), u32> {
        let inode = fs.inode(id).ok_or(ESTALE)?;
        reply.u64(mask & 0x7ff)?;
        reply.qid(inode)?;
        reply.u32(Self::mode(inode))?;
        reply.u32(inode.uid)?;
        reply.u32(inode.gid)?;
        let links = match &inode.kind {
            InodeKind::Directory { entries, .. } if inode.link_count != 0 => {
                2 + u64::try_from(
                    entries
                        .values()
                        .filter(|entry| {
                            fs.inode(entry.inode).is_some_and(|child| {
                                matches!(child.kind, InodeKind::Directory { .. })
                            })
                        })
                        .count(),
                )
                .expect("bounded entries")
            }
            _ => u64::from(inode.link_count),
        };
        let size = u64::try_from(inode.size()).expect("bounded inode size");
        reply.u64(links)?;
        reply.u64(0)?;
        reply.u64(size)?;
        reply.u64(4096)?;
        reply.u64(size.div_ceil(512))?;
        for (seconds, ns) in [
            (inode.atime, inode.atime_nanoseconds),
            (inode.mtime, inode.mtime_nanoseconds),
            (inode.ctime, inode.ctime_nanoseconds),
        ] {
            reply.u64(seconds)?;
            reply.u64(u64::from(ns))?;
        }
        reply.u64(0)?;
        reply.u64(0)?;
        reply.u64(0)?;
        reply.u64(0)
    }

    fn mode(inode: &Inode) -> u32 {
        inode.mode
            | match inode.kind {
                InodeKind::Directory { .. } => 0o040_000,
                InodeKind::File(_) => 0o100_000,
                InodeKind::Symlink(_) => 0o120_000,
            }
    }

    fn readdir(
        &self,
        fs: &Filesystem,
        number: u32,
        offset: u64,
        count: u32,
        reply: &mut Writer,
    ) -> Result<(), u32> {
        let (fid, flags) = self.opened(number)?;
        if !flags.readable() {
            return Err(EBADF);
        }
        let maximum = usize::try_from(count)
            .map_err(|_| EINVAL)?
            .min(reply.remaining().saturating_sub(4));
        reply.u32(0)?;
        let start = reply.bytes.len();
        for entry in fs.directory_entries(fid.inode).map_err(Self::errno)? {
            if entry.cookie <= offset {
                continue;
            }
            let size = 24 + entry.name.len();
            if reply.bytes.len() - start + size > maximum {
                break;
            }
            let inode = fs.inode(entry.inode).ok_or(ESTALE)?;
            reply.qid(inode)?;
            reply.u64(entry.cookie)?;
            reply.u8(match inode.kind {
                InodeKind::Directory { .. } => 4,
                InodeKind::File(_) => 8,
                InodeKind::Symlink(_) => 10,
            })?;
            reply.string(&entry.name)?;
        }
        let count = u32::try_from(reply.bytes.len() - start).expect("bounded directory reply");
        reply.bytes[7..11].copy_from_slice(&count.to_le_bytes());
        Ok(())
    }

    fn lock_request(
        &self,
        fs: &Filesystem,
        raw: Lock<'_>,
        check_access: bool,
    ) -> Result<ByteRangeLock, u32> {
        let (fid, flags) = self.opened(raw.fid)?;
        if !matches!(fs.inode(fid.inode).ok_or(ESTALE)?.kind, InodeKind::File(_)) {
            return Err(EOPNOTSUPP);
        }
        let kind = match raw.kind {
            0 if flags.readable() || !check_access => LockKind::Read,
            1 if flags.writable() || !check_access => LockKind::Write,
            0 | 1 => return Err(EBADF),
            2 => LockKind::Read,
            _ => return Err(EINVAL),
        };
        Ok(ByteRangeLock {
            inode: fid.inode,
            session: self.session,
            kind,
            start: raw.start,
            length: raw.length,
            process: raw.process,
            client: raw.client.to_owned(),
        })
    }

    fn lock(&self, fs: &mut Filesystem, raw: Lock<'_>, reply: &mut Writer) -> Result<(), u32> {
        if raw.flags & !1 != 0 {
            return Err(EOPNOTSUPP);
        }
        let lock = self.lock_request(fs, raw, true)?;
        let acquired = if raw.kind == 2 {
            fs.unlock(
                lock.inode,
                self.session,
                lock.process,
                &lock.client,
                lock.start,
                lock.length,
            )
            .map_err(Self::errno)?;
            true
        } else {
            fs.try_lock(lock).map_err(Self::errno)?
        };
        reply.u8(u8::from(!acquired))
    }

    fn getlock(&self, fs: &Filesystem, raw: Lock<'_>, reply: &mut Writer) -> Result<(), u32> {
        if raw.kind == 2 {
            return Err(EINVAL);
        }
        let requested = self.lock_request(fs, raw, false)?;
        if let Some(held) = fs.conflicting_lock(&requested).map_err(Self::errno)? {
            reply.u8(match held.kind {
                LockKind::Read => 0,
                LockKind::Write => 1,
            })?;
            reply.u64(held.start)?;
            reply.u64(held.length)?;
            reply.u32(held.process)?;
            reply.string(&held.client)
        } else {
            // F_GETLK changes the type to UNLCK and leaves other fields intact.
            reply.u8(2)?;
            reply.u64(raw.start)?;
            reply.u64(raw.length)?;
            reply.u32(raw.process)?;
            reply.string(raw.client)
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, size: usize) -> Result<&'a [u8], u32> {
        let end = self.offset.checked_add(size).ok_or(EPROTO)?;
        let bytes = self.bytes.get(self.offset..end).ok_or(EPROTO)?;
        self.offset = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, u32> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, u32> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }
    fn u32(&mut self) -> Result<u32, u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    fn u64(&mut self) -> Result<u64, u32> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    fn string(&mut self) -> Result<&'a str, u32> {
        let size = usize::from(self.u16()?);
        let value = from_utf8(self.take(size)?).map_err(|_| EPROTO)?;
        if value.contains('\0') {
            return Err(EPROTO);
        }
        Ok(value)
    }

    fn done(&self) -> Result<(), u32> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(EPROTO)
        }
    }
}

struct Writer {
    bytes: Vec<u8>,
    capacity: usize,
}

impl Writer {
    fn remaining(&self) -> usize {
        self.capacity - self.bytes.len()
    }
    fn new(kind: u8, tag: u16, capacity: usize) -> Result<Self, u32> {
        if capacity < 7 {
            return Err(ENOSPC);
        }
        let mut result = Self {
            bytes: vec![0; 4],
            capacity,
        };
        result.u8(kind)?;
        result.u16(tag)?;
        Ok(result)
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), u32> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|end| end > self.capacity)
        {
            return Err(ENOSPC);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn u8(&mut self, value: u8) -> Result<(), u32> {
        self.append(&[value])
    }
    fn u16(&mut self, value: u16) -> Result<(), u32> {
        self.append(&value.to_le_bytes())
    }
    fn u32(&mut self, value: u32) -> Result<(), u32> {
        self.append(&value.to_le_bytes())
    }
    fn u64(&mut self, value: u64) -> Result<(), u32> {
        self.append(&value.to_le_bytes())
    }

    fn string(&mut self, value: &str) -> Result<(), u32> {
        self.u16(u16::try_from(value.len()).map_err(|_| ENOSPC)?)?;
        self.append(value.as_bytes())
    }

    fn qid(&mut self, inode: &Inode) -> Result<(), u32> {
        self.u8(match inode.kind {
            InodeKind::Directory { .. } => 0x80,
            InodeKind::Symlink(_) => 2,
            InodeKind::File(_) => 0,
        })?;
        self.u32(inode.version)?;
        self.u64(inode.id.0)
    }

    fn finish(mut self) -> Vec<u8> {
        let length = u32::try_from(self.bytes.len()).expect("bounded message size");
        self.bytes[..4].copy_from_slice(&length.to_le_bytes());
        self.bytes
    }
}

#[derive(Clone, Copy, Debug)]
struct Attributes {
    mask: u32,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    atime: u64,
    atime_ns: u64,
    mtime: u64,
    mtime_ns: u64,
}

#[derive(Clone, Copy, Debug)]
struct Lock<'a> {
    fid: u32,
    kind: u8,
    flags: u32,
    start: u64,
    length: u64,
    process: u32,
    client: &'a str,
}

#[derive(Clone, Copy)]
struct Creation<'a> {
    name: &'a str,
    flags: u32,
    mode: u32,
    gid: u32,
}

enum Request<'a> {
    Version {
        size: u32,
        version: &'a str,
    },
    Attach {
        fid: u32,
        afid: u32,
        uid: u32,
    },
    Flush,
    Walk {
        fid: u32,
        new_fid: u32,
        names: Vec<&'a str>,
    },
    Open {
        fid: u32,
        flags: u32,
    },
    Create {
        fid: u32,
        file: Creation<'a>,
    },
    Read {
        fid: u32,
        offset: u64,
        count: u32,
    },
    Write {
        fid: u32,
        offset: u64,
        data: &'a [u8],
    },
    Clunk {
        fid: u32,
    },
    Statfs {
        fid: u32,
    },
    Getattr {
        fid: u32,
        mask: u64,
    },
    Setattr {
        fid: u32,
        attributes: Attributes,
    },
    Readdir {
        fid: u32,
        offset: u64,
        count: u32,
    },
    Fsync {
        fid: u32,
        data_only: u32,
    },
    Symlink {
        fid: u32,
        name: &'a str,
        target: &'a str,
        gid: u32,
    },
    Readlink {
        fid: u32,
    },
    Mkdir {
        fid: u32,
        name: &'a str,
        mode: u32,
        gid: u32,
    },
    Link {
        directory: u32,
        fid: u32,
        name: &'a str,
    },
    Rename {
        old_directory: u32,
        old_name: &'a str,
        new_directory: u32,
        new_name: &'a str,
    },
    Unlink {
        directory: u32,
        name: &'a str,
        flags: u32,
    },
    Lock(Lock<'a>),
    Getlock(Lock<'a>),
    Unsupported,
}

impl<'a> Request<'a> {
    // Borrowed strings and write bodies stay within the current activation.
    fn parse(kind: u8, bytes: &'a [u8]) -> Result<Self, u32> {
        let mut r = Reader::new(bytes);
        let result = match kind {
            100 => Self::Version {
                size: r.u32()?,
                version: r.string()?,
            },
            104 => {
                let fid = r.u32()?;
                let afid = r.u32()?;
                r.string()?;
                r.string()?;
                Self::Attach {
                    fid,
                    afid,
                    uid: r.u32()?,
                }
            }
            108 => {
                r.u16()?;
                Self::Flush
            }
            110 => {
                let fid = r.u32()?;
                let new_fid = r.u32()?;
                let count = usize::from(r.u16()?);
                if count > 16 {
                    return Err(EINVAL);
                }
                let mut names = Vec::with_capacity(count);
                for _ in 0..count {
                    names.push(r.string()?);
                }
                Self::Walk {
                    fid,
                    new_fid,
                    names,
                }
            }
            12 => Self::Open {
                fid: r.u32()?,
                flags: r.u32()?,
            },
            14 => Self::Create {
                fid: r.u32()?,
                file: Creation {
                    name: r.string()?,
                    flags: r.u32()?,
                    mode: r.u32()?,
                    gid: r.u32()?,
                },
            },
            116 => Self::Read {
                fid: r.u32()?,
                offset: r.u64()?,
                count: r.u32()?,
            },
            118 => {
                let fid = r.u32()?;
                let offset = r.u64()?;
                let count = usize::try_from(r.u32()?).map_err(|_| EPROTO)?;
                Self::Write {
                    fid,
                    offset,
                    data: r.take(count)?,
                }
            }
            120 => Self::Clunk { fid: r.u32()? },
            8 => Self::Statfs { fid: r.u32()? },
            24 => Self::Getattr {
                fid: r.u32()?,
                mask: r.u64()?,
            },
            26 => Self::Setattr {
                fid: r.u32()?,
                attributes: Attributes {
                    mask: r.u32()?,
                    mode: r.u32()?,
                    uid: r.u32()?,
                    gid: r.u32()?,
                    size: r.u64()?,
                    atime: r.u64()?,
                    atime_ns: r.u64()?,
                    mtime: r.u64()?,
                    mtime_ns: r.u64()?,
                },
            },
            40 => Self::Readdir {
                fid: r.u32()?,
                offset: r.u64()?,
                count: r.u32()?,
            },
            50 => Self::Fsync {
                fid: r.u32()?,
                data_only: r.u32()?,
            },
            16 => Self::Symlink {
                fid: r.u32()?,
                name: r.string()?,
                target: r.string()?,
                gid: r.u32()?,
            },
            22 => Self::Readlink { fid: r.u32()? },
            72 => Self::Mkdir {
                fid: r.u32()?,
                name: r.string()?,
                mode: r.u32()?,
                gid: r.u32()?,
            },
            70 => Self::Link {
                directory: r.u32()?,
                fid: r.u32()?,
                name: r.string()?,
            },
            74 => Self::Rename {
                old_directory: r.u32()?,
                old_name: r.string()?,
                new_directory: r.u32()?,
                new_name: r.string()?,
            },
            76 => Self::Unlink {
                directory: r.u32()?,
                name: r.string()?,
                flags: r.u32()?,
            },
            52 | 54 => {
                let lock = Lock {
                    fid: r.u32()?,
                    kind: r.u8()?,
                    flags: if kind == 52 { r.u32()? } else { 0 },
                    start: r.u64()?,
                    length: r.u64()?,
                    process: r.u32()?,
                    client: r.string()?,
                };
                if kind == 52 {
                    Self::Lock(lock)
                } else {
                    Self::Getlock(lock)
                }
            }
            _ => return Ok(Self::Unsupported),
        };
        r.done()?;
        Ok(result)
    }

    const fn minimum_reply(&self) -> usize {
        match self {
            Self::Open { .. } | Self::Create { .. } => 24,
            Self::Attach { .. } | Self::Symlink { .. } | Self::Mkdir { .. } => 20,
            Self::Walk { .. } => 9,
            Self::Statfs { .. } => 67,
            Self::Getattr { .. } => 160,
            Self::Read { .. } | Self::Write { .. } | Self::Readdir { .. } => 11,
            Self::Getlock(_) => 30,
            Self::Lock(_) => 8,
            _ => 7,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Wire-level regressions use independent packet builders and observable replies.

    use super::{NinePSession, ProtocolError};
    use crate::ninep::{ChangeSource, Filesystem, Limits};

    #[derive(Default)]
    struct Body(Vec<u8>);
    impl Body {
        fn u8(mut self, value: u8) -> Self {
            self.0.push(value);
            self
        }
        fn u16(mut self, value: u16) -> Self {
            self.0.extend(value.to_le_bytes());
            self
        }
        fn u32(mut self, value: u32) -> Self {
            self.0.extend(value.to_le_bytes());
            self
        }
        fn u64(mut self, value: u64) -> Self {
            self.0.extend(value.to_le_bytes());
            self
        }
        fn string(mut self, value: &str) -> Self {
            self.0
                .extend(u16::try_from(value.len()).unwrap().to_le_bytes());
            self.0.extend(value.as_bytes());
            self
        }
        fn bytes(mut self, value: &[u8]) -> Self {
            self.0.extend(value);
            self
        }
        fn into_packet(self, kind: u8, tag: u16) -> Vec<u8> {
            let mut bytes = Body::default()
                .u32(u32::try_from(self.0.len() + 7).unwrap())
                .u8(kind)
                .u16(tag)
                .0;
            bytes.extend(self.0);
            bytes
        }
    }
    fn packet(kind: u8, tag: u16, body: &Body) -> Vec<u8> {
        Body::default()
            .u32(u32::try_from(7 + body.0.len()).unwrap())
            .u8(kind)
            .u16(tag)
            .bytes(&body.0)
            .0
    }
    fn at32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }
    fn at64(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
    }
    fn error(bytes: &[u8], errno: u32) {
        assert_eq!((bytes[4], at32(bytes, 7)), (7, errno));
    }
    fn version(size: u32, name: &str) -> Body {
        Body::default().u32(size).string(name)
    }
    fn attach(fid: u32) -> Body {
        Body::default()
            .u32(fid)
            .u32(u32::MAX)
            .string("")
            .string("")
            .u32(123)
    }
    fn walk(fid: u32, new: u32, names: &[&str]) -> Body {
        let mut body = Body::default()
            .u32(fid)
            .u32(new)
            .u16(u16::try_from(names.len()).unwrap());
        for name in names {
            body = body.string(name);
        }
        body
    }
    fn read(fid: u32, offset: u64, count: u32) -> Body {
        Body::default().u32(fid).u64(offset).u32(count)
    }
    fn write(fid: u32, offset: u64, data: &[u8]) -> Body {
        read(fid, offset, u32::try_from(data.len()).unwrap()).bytes(data)
    }
    fn attributes(fid: u32, mask: u32, size: u64, seconds: u64, ns: u64) -> Body {
        Body::default()
            .u32(fid)
            .u32(mask)
            .u32(0o600)
            .u32(77)
            .u32(88)
            .u64(size)
            .u64(seconds)
            .u64(ns)
            .u64(seconds)
            .u64(ns)
    }
    fn lock(fid: u32, kind: u8, flags: Option<u32>, start: u64, length: u64, process: u32) -> Body {
        let mut body = Body::default().u32(fid).u8(kind);
        if let Some(flags) = flags {
            body = body.u32(flags);
        }
        body.u64(start).u64(length).u32(process).string("browser")
    }

    struct Client {
        session: NinePSession,
        tag: u16,
    }
    impl Client {
        fn mount(fs: &mut Filesystem) -> Self {
            let mut client = Self {
                session: NinePSession::new(fs).unwrap(),
                tag: 1,
            };
            let reply = client.call_tag(fs, 100, u16::MAX, version(8192, "9P2000.L"), 8192);
            assert_eq!(reply[4], 101);
            assert_eq!(client.call(fs, 104, attach(1))[4], 105);
            client
        }

        fn submit_packet(
            &mut self,
            fs: &mut Filesystem,
            bytes: &[u8],
            capacity: usize,
        ) -> Result<Vec<u8>, ProtocolError> {
            self.session.submit(fs, bytes, capacity)
        }
        fn call_tag(
            &mut self,
            fs: &mut Filesystem,
            kind: u8,
            tag: u16,
            body: Body,
            capacity: usize,
        ) -> Vec<u8> {
            let reply = self
                .submit_packet(fs, &body.into_packet(kind, tag), capacity)
                .unwrap();
            assert_eq!(usize::try_from(at32(&reply, 0)).unwrap(), reply.len());
            assert_eq!(u16::from_le_bytes(reply[5..7].try_into().unwrap()), tag);
            assert!(reply.len() <= capacity);
            reply
        }
        fn call(&mut self, fs: &mut Filesystem, kind: u8, body: Body) -> Vec<u8> {
            let tag = self.tag;
            self.tag += 1;
            self.call_tag(fs, kind, tag, body, 8192)
        }
        fn open(&mut self, fs: &mut Filesystem, fid: u32, name: &str, flags: u32) {
            assert_eq!(self.call(fs, 110, walk(1, fid, &[name]))[4], 111);
            assert_eq!(
                self.call(fs, 12, Body::default().u32(fid).u32(flags))[4],
                13
            );
        }
    }
    fn filesystem() -> Filesystem {
        Filesystem::new(Limits::default(), 100)
    }

    // Walk must preserve both fids on partial success, including an in-place walk.
    // A small reply buffer must not install the result of an otherwise valid walk.
    #[test]
    fn walks_return_partial_qids_without_installing_or_replacing_fids() {
        let mut fs = filesystem();
        fs.mkdir("dir").unwrap();
        fs.write_file("dir/file", b"x").unwrap();
        let mut c = Client::mount(&mut fs);
        let reply = c.call(&mut fs, 110, walk(1, 2, &["dir", "missing"]));
        assert_eq!((reply[4], reply.len()), (111, 22));
        error(&c.call(&mut fs, 12, Body::default().u32(2).u32(0)), 9);
        assert_eq!(
            c.call(&mut fs, 110, walk(1, 1, &["dir", "missing"]))[4],
            111
        );
        assert_eq!(c.call(&mut fs, 110, walk(1, 2, &["dir", "file"]))[4], 111);
        error(&c.call(&mut fs, 110, walk(2, 3, &[".."])), 20);
        let reply = c.call_tag(&mut fs, 110, 100, walk(1, 3, &["dir", "file"]), 22);
        error(&reply, 28);
        error(&c.call(&mut fs, 12, Body::default().u32(3).u32(0)), 9);
        error(&c.call(&mut fs, 110, walk(1, 3, &["dir"; 17])), 22);
        assert_eq!(c.call(&mut fs, 110, walk(2, 3, &[]))[4], 111);
        assert_eq!(c.call(&mut fs, 12, Body::default().u32(3).u32(0))[4], 13);
        error(&c.call(&mut fs, 110, walk(3, 4, &[])), 9);
    }

    #[test]
    fn malformed_envelopes_keep_the_tag_and_reply_limits_and_do_not_create_files() {
        let mut fs = filesystem();
        let mut c = Client::mount(&mut fs);
        let mut bytes = packet(
            72,
            444,
            &Body::default().u32(1).string("dir").u32(0o755).u32(1),
        );
        bytes[0] = 1;
        let reply = c.session.submit(&mut fs, &bytes, 11).unwrap();
        error(&reply, 71);
        assert_eq!(&reply[5..7], &444_u16.to_le_bytes());
        assert!(fs.lookup("dir").is_err());
        let body = Body::default()
            .u32(1)
            .string("file")
            .u32(2)
            .u32(0o600)
            .u32(9);
        error(&c.call_tag(&mut fs, 14, 445, body, 20), 28);
        assert!(fs.lookup("file").is_err());
        assert_eq!(
            c.session.submit(&mut fs, &[0; 6], 100),
            Err(ProtocolError::Malformed)
        );
        let body = attach(2).bytes(&[0]);
        error(&c.call(&mut fs, 104, body), 71);
        assert_eq!(c.call(&mut fs, 104, attach(2))[4], 105);
    }

    #[test]
    fn create_range_io_append_and_clunk_preserve_inode_content_and_ownership() {
        let mut fs = filesystem();
        let mut c = Client::mount(&mut fs);
        c.call(&mut fs, 110, walk(1, 2, &[]));
        let created = c.call(
            &mut fs,
            14,
            Body::default()
                .u32(2)
                .string("file")
                .u32(2)
                .u32(0o640)
                .u32(9),
        );
        assert_eq!(created[4], 15);
        let id = fs.lookup("file").unwrap();
        assert_eq!(
            (fs.inode(id).unwrap().uid, fs.inode(id).unwrap().gid),
            (123, 9)
        );
        assert_eq!(at32(&c.call(&mut fs, 118, write(2, 2, b"xy")), 7), 2);
        assert_eq!(&c.call(&mut fs, 116, read(2, 0, 99))[11..], b"\0\0xy");
        assert_eq!(at32(&c.call(&mut fs, 116, read(2, u64::MAX, 99)), 7), 0);
        error(&c.call(&mut fs, 118, write(2, u64::MAX, b"x")), 27);
        c.open(&mut fs, 3, "file", 0x402);
        c.call(&mut fs, 118, write(3, 0, b"z"));
        assert_eq!(fs.read_file("file"), Ok(b"\0\0xyz".to_vec()));
        assert_eq!(c.call(&mut fs, 50, Body::default().u32(3).u32(1))[4], 51);
        assert_eq!(c.call(&mut fs, 120, Body::default().u32(3))[4], 121);
        error(&c.call(&mut fs, 116, read(3, 0, 1)), 9);
    }

    #[test]
    fn open_access_modes_truncation_symlinks_and_directory_rules_are_enforced() {
        let mut fs = filesystem();
        fs.write_file("file", b"old").unwrap();
        fs.symlink("link", "file").unwrap();
        let mut c = Client::mount(&mut fs);
        c.open(&mut fs, 2, "file", 0);
        error(&c.call(&mut fs, 118, write(2, 0, b"x")), 9);
        error(&c.call(&mut fs, 12, Body::default().u32(2).u32(0)), 16);
        c.open(&mut fs, 3, "file", 0x201);
        assert_eq!(fs.read_file("file"), Ok(Vec::new()));
        error(&c.call(&mut fs, 116, read(3, 0, 1)), 9);
        c.call(&mut fs, 110, walk(1, 4, &["link"]));
        error(&c.call(&mut fs, 12, Body::default().u32(4).u32(0)), 40);
        error(&c.call(&mut fs, 12, Body::default().u32(1).u32(2)), 21);
        error(
            &c.call(&mut fs, 12, Body::default().u32(1).u32(0x0020_0000)),
            95,
        );
        c.call(&mut fs, 110, walk(1, 5, &["file"]));
        error(
            &c.call(&mut fs, 12, Body::default().u32(5).u32(0x10000)),
            20,
        );
    }

    #[test]
    fn setattr_is_atomic_preserves_unselected_fields_and_retains_nanoseconds() {
        let mut fs = Filesystem::new(
            Limits {
                max_file_bytes: 4,
                ..Limits::default()
            },
            100,
        );
        let id = fs.write_file("file", b"abc").unwrap();
        let mut c = Client::mount(&mut fs);
        c.open(&mut fs, 2, "file", 2);
        error(&c.call(&mut fs, 26, attributes(2, 1 | 8, 5, 0, 0)), 27);
        assert_eq!(fs.inode(id).unwrap().mode, 0o644);
        error(
            &c.call(&mut fs, 26, attributes(2, 0x120, 0, 55, 1_000_000_000)),
            22,
        );
        assert_eq!(fs.inode(id).unwrap().mtime, 100);
        fs.set_time(200);
        c.call(
            &mut fs,
            26,
            attributes(2, 0x1b7, 0, u64::MAX - 1, 999_999_999),
        );
        let reply = c.call(&mut fs, 24, Body::default().u32(2).u64(u64::MAX));
        assert_eq!((reply.len(), at64(&reply, 7)), (160, 0x7ff));
        assert_eq!(
            (at32(&reply, 28), at32(&reply, 32), at32(&reply, 36)),
            (0o100_600, 77, 88)
        );
        assert_eq!(
            (at64(&reply, 80), at64(&reply, 88)),
            (u64::MAX - 1, 999_999_999)
        );
        assert_eq!(
            (at64(&reply, 96), at64(&reply, 104)),
            (u64::MAX - 1, 999_999_999)
        );
        assert_eq!(at64(&reply, 112), 200);
        c.call(&mut fs, 26, attributes(2, 1, 0, 0, u64::MAX));
        assert_eq!(fs.inode(id).unwrap().mtime, u64::MAX - 1);
        c.call(&mut fs, 26, attributes(2, 8, 4, 0, 0));
        assert_eq!(fs.read_file("file"), Ok(b"abc\0".to_vec()));
    }

    #[test]
    fn directory_operations_links_renames_and_unlinks_preserve_open_inodes() {
        let mut fs = filesystem();
        fs.write_file("file", b"data").unwrap();
        let mut c = Client::mount(&mut fs);
        c.open(&mut fs, 2, "file", 2);
        assert_eq!(
            c.call(
                &mut fs,
                72,
                Body::default().u32(1).string("dir").u32(0o750).u32(9)
            )[4],
            73
        );
        c.call(&mut fs, 110, walk(1, 3, &["dir"]));
        assert_eq!(
            c.call(&mut fs, 70, Body::default().u32(3).u32(2).string("alias"))[4],
            71
        );
        c.call(
            &mut fs,
            74,
            Body::default()
                .u32(3)
                .string("alias")
                .u32(1)
                .string("moved"),
        );
        assert_eq!(fs.lookup("file"), fs.lookup("moved"));
        c.call(
            &mut fs,
            16,
            Body::default()
                .u32(3)
                .string("link")
                .string("../moved")
                .u32(9),
        );
        c.call(&mut fs, 110, walk(1, 4, &["dir", "link"]));
        assert_eq!(
            &c.call(&mut fs, 22, Body::default().u32(4))[9..],
            b"../moved"
        );
        error(
            &c.call(&mut fs, 76, Body::default().u32(1).string("dir").u32(0)),
            21,
        );
        error(
            &c.call(&mut fs, 76, Body::default().u32(1).string("dir").u32(0x200)),
            39,
        );
        c.call(&mut fs, 76, Body::default().u32(1).string("file").u32(0));
        c.call(&mut fs, 76, Body::default().u32(1).string("moved").u32(0));
        c.call(&mut fs, 118, write(2, 0, b"open"));
        assert_eq!(&c.call(&mut fs, 116, read(2, 0, 4))[11..], b"open");
        c.call(
            &mut fs,
            70,
            Body::default().u32(1).u32(2).string("restored"),
        );
        assert_eq!(fs.read_file("restored"), Ok(b"open".to_vec()));
        error(
            &c.call(&mut fs, 70, Body::default().u32(1).u32(3).string("bad")),
            1,
        );
    }

    #[test]
    fn readdir_caps_records_and_cookies_survive_mutation_between_pages() {
        let mut fs = filesystem();
        fs.write_file("a", b"").unwrap();
        fs.write_file("b", b"").unwrap();
        let mut c = Client::mount(&mut fs);
        c.call(&mut fs, 110, walk(1, 2, &[]));
        error(&c.call(&mut fs, 40, read(2, 0, 100)), 9);
        c.call(&mut fs, 12, Body::default().u32(2).u32(0));
        let first = c.call_tag(&mut fs, 40, 100, read(2, 0, u32::MAX), 36);
        assert_eq!(at32(&first, 7), 25);
        assert_eq!(&first[35..], b"a");
        let cookie = at64(&first, 24);
        fs.remove("a").unwrap();
        fs.write_file("c", b"").unwrap();
        let next = c.call(&mut fs, 40, read(2, cookie, 25));
        assert_eq!(&next[35..], b"b");
        let cookie = at64(&next, 24);
        let last = c.call(&mut fs, 40, read(2, cookie, 25));
        assert_eq!(&last[35..], b"c");
        assert_eq!(
            at32(&c.call(&mut fs, 40, read(2, at64(&last, 24), 100)), 7),
            0
        );
        assert_eq!(at32(&c.call(&mut fs, 40, read(2, 0, 24)), 7), 0);
        error(&c.call(&mut fs, 116, read(2, 0, 1)), 21);
    }

    #[test]
    fn locks_report_owner_and_split_ranges_and_session_close_releases_them() {
        let mut fs = filesystem();
        fs.write_file("file", b"x").unwrap();
        let mut a = Client::mount(&mut fs);
        let mut b = Client::mount(&mut fs);
        a.open(&mut fs, 2, "file", 2);
        b.open(&mut fs, 2, "file", 0);
        assert_eq!(a.call(&mut fs, 52, lock(2, 1, Some(0), 0, 100, 10))[7], 0);
        assert_eq!(a.call(&mut fs, 52, lock(2, 1, Some(1), 0, 100, 20))[7], 1);
        let held = b.call(&mut fs, 54, lock(2, 1, None, 0, 100, 20));
        assert_eq!(
            (held[7], at64(&held, 8), at64(&held, 16), at32(&held, 24)),
            (1, 0, 100, 10)
        );
        assert_eq!(&held[30..], b"browser");
        a.call(&mut fs, 52, lock(2, 2, Some(0), 40, 20, 10));
        assert_eq!(b.call(&mut fs, 54, lock(2, 1, None, 40, 20, 20))[7], 2);
        assert_eq!(b.call(&mut fs, 54, lock(2, 1, None, 60, 20, 20))[7], 1);
        a.session.close(&mut fs).unwrap();
        assert_eq!(b.call(&mut fs, 54, lock(2, 1, None, 0, 100, 20))[7], 2);
    }

    #[test]
    fn malformed_mutating_operations_leave_namespace_and_fid_state_unchanged() {
        let cases = [
            (
                14,
                Body::default()
                    .u32(1)
                    .string("new")
                    .u32(2)
                    .u32(0o600)
                    .u32(9),
            ),
            (118, write(2, 0, b"x")),
            (26, attributes(2, 1 | 8, 0, 0, 0)),
            (120, Body::default().u32(2)),
            (110, walk(1, 4, &["file"])),
            (70, Body::default().u32(1).u32(2).string("alias")),
            (
                74,
                Body::default().u32(1).string("file").u32(1).string("moved"),
            ),
            (76, Body::default().u32(1).string("file").u32(0)),
            (
                16,
                Body::default().u32(1).string("link").string("file").u32(9),
            ),
            (72, Body::default().u32(1).string("dir").u32(0o755).u32(9)),
        ];
        for (kind, body) in cases {
            let mut fs = filesystem();
            let id = fs.write_file("file", b"old").unwrap();
            let mut c = Client::mount(&mut fs);
            c.open(&mut fs, 2, "file", 2);
            let before = fs.inode(id).unwrap().clone();
            error(&c.call(&mut fs, kind, body.bytes(&[0])), 71);
            assert_eq!(fs.inode(id).unwrap(), &before, "operation {kind}");
            assert_eq!(fs.list_files(), vec!["file"]);
            assert_eq!(&c.call(&mut fs, 116, read(2, 0, 3))[11..], b"old");
        }
    }

    #[test]
    fn every_truncated_setattr_is_rejected_before_any_selected_field_changes() {
        let mut fs = filesystem();
        let id = fs.write_file("file", b"old").unwrap();
        let mut c = Client::mount(&mut fs);
        c.open(&mut fs, 2, "file", 2);
        let before = fs.inode(id).unwrap().clone();
        let body = attributes(2, 0x1ff, 0, 10, 0);
        for length in 0..body.0.len() {
            let truncated = Body(body.0[..length].to_vec());
            error(&c.call(&mut fs, 26, truncated), 71);
            assert_eq!(fs.inode(id).unwrap(), &before);
        }
    }

    #[test]
    fn statfs_masks_and_unsupported_operations_do_not_claim_extra_features() {
        let mut fs = filesystem();
        let mut c = Client::mount(&mut fs);
        let statfs = c.call(&mut fs, 8, Body::default().u32(1));
        assert_eq!((statfs.len(), at32(&statfs, 63)), (67, 255));
        for kind in [18, 20, 30, 32, 102, 122, 250] {
            error(&c.call(&mut fs, kind, Body::default()), 95);
        }
        error(&c.call(&mut fs, 104, attach(u32::MAX)), 22);
        error(
            &c.call(
                &mut fs,
                72,
                Body::default()
                    .u32(1)
                    .string(&"x".repeat(256))
                    .u32(0o755)
                    .u32(9),
            ),
            36,
        );
        while fs.next_change().is_some() {}
        c.call(
            &mut fs,
            72,
            Body::default().u32(1).string("dir").u32(0o755).u32(9),
        );
        assert_eq!(fs.next_change().unwrap().source, ChangeSource::Guest);
        fs.write_file("host", b"x").unwrap();
        assert_eq!(fs.next_change().unwrap().source, ChangeSource::Host);
        let reply = c.call(&mut fs, 24, Body::default().u32(1).u64(0x3fff));
        assert_eq!(at64(&reply, 7), 0x7ff);
        assert_eq!(at64(&reply, 40), 3);
    }

    #[test]
    fn create_existing_files_obeys_exclusive_and_truncate_flags() {
        let mut fs = filesystem();
        let inode = fs.write_file("file", b"old").unwrap();
        let mut c = Client::mount(&mut fs);
        let creation = |flags| {
            Body::default()
                .u32(1)
                .string("file")
                .u32(flags)
                .u32(0o600)
                .u32(77)
        };
        error(&c.call(&mut fs, 14, creation(2 | 0x80)), 17);
        assert_eq!(c.call(&mut fs, 14, creation(2))[4], 15);
        assert_eq!(&c.call(&mut fs, 116, read(1, 0, 3))[11..], b"old");
        assert_eq!(fs.lookup("file"), Ok(inode));
        assert_eq!(
            (fs.inode(inode).unwrap().mode, fs.inode(inode).unwrap().gid),
            (0o644, 1000)
        );
        c.call(&mut fs, 104, attach(2));
        let body = Body::default()
            .u32(2)
            .string("file")
            .u32(2 | 0x200)
            .u32(0o600)
            .u32(77);
        assert_eq!(c.call(&mut fs, 14, body)[4], 15);
        assert_eq!(fs.read_file("file"), Ok(Vec::new()));
    }

    #[test]
    fn negotiated_and_descriptor_bounds_limit_reads_before_any_mutation() {
        let mut fs = filesystem();
        fs.write_file("file", &[1; 1024]).unwrap();
        let mut c = Client::mount(&mut fs);
        c.call_tag(&mut fs, 100, u16::MAX, version(256, "9P2000.L"), 100);
        c.call(&mut fs, 104, attach(1));
        c.open(&mut fs, 2, "file", 2);
        let reply = c.call(&mut fs, 116, read(2, 0, u32::MAX));
        assert_eq!((reply.len(), at32(&reply, 7)), (256, 245));
        let reply = c.call_tag(&mut fs, 116, 100, read(2, 0, 100), 16);
        assert_eq!((reply.len(), at32(&reply, 7)), (16, 5));
        c.call(&mut fs, 110, walk(1, 3, &["file"]));
        let reply = c.call_tag(&mut fs, 12, 101, Body::default().u32(3).u32(0x202), 23);
        error(&reply, 28);
        assert_eq!(fs.inode(fs.lookup("file").unwrap()).unwrap().size(), 1024);
        let reply = c.call(
            &mut fs,
            104,
            Body::default()
                .u32(4)
                .u32(u32::MAX)
                .string(&"a".repeat(256))
                .string("")
                .u32(1),
        );
        error(&reply, 71);
        c.call_tag(&mut fs, 100, u16::MAX, version(8192, "unknown"), 100);
        error(&c.call(&mut fs, 104, attach(1)), 71);
        c.call_tag(&mut fs, 100, u16::MAX, version(8192, "9P2000.L"), 100);
        assert_eq!(c.call(&mut fs, 104, attach(1))[4], 105);
    }

    #[test]
    fn unicode_names_round_trip_and_invalid_utf8_is_rejected_before_creation() {
        let mut fs = filesystem();
        let mut c = Client::mount(&mut fs);
        c.call(&mut fs, 110, walk(1, 2, &[]));
        let body = Body::default()
            .u32(2)
            .string("π.c")
            .u32(2)
            .u32(0o644)
            .u32(9);
        assert_eq!(c.call(&mut fs, 14, body)[4], 15);
        c.call(&mut fs, 118, write(2, 0, "λ".as_bytes()));
        assert_eq!(fs.read_file("π.c"), Ok("λ".as_bytes().to_vec()));
        let invalid = Body::default()
            .u32(1)
            .u16(1)
            .bytes(&[0xff])
            .u32(0o755)
            .u32(9);
        error(&c.call(&mut fs, 72, invalid), 71);
        assert_eq!(fs.list_files(), vec!["π.c"]);
        c.call(&mut fs, 110, walk(1, 3, &[]));
        c.call(&mut fs, 12, Body::default().u32(3).u32(0));
        let entries = c.call(&mut fs, 40, read(3, 0, 100));
        assert_eq!(&entries[35..], "π.c".as_bytes());
    }

    #[test]
    fn clear_preserves_sessions_open_files_directory_fids_and_locks() {
        let mut fs = filesystem();
        let directory = fs.mkdir("dir").unwrap();
        let file = fs.write_file("dir/file", b"old").unwrap();
        let mut first = Client::mount(&mut fs);
        let mut second = Client::mount(&mut fs);
        assert_eq!(first.call(&mut fs, 110, walk(1, 2, &["dir"]))[4], 111);
        assert_eq!(first.call(&mut fs, 110, walk(2, 3, &["file"]))[4], 111);
        assert_eq!(
            first.call(&mut fs, 12, Body::default().u32(2).u32(0))[4],
            13
        );
        assert_eq!(
            first.call(&mut fs, 12, Body::default().u32(3).u32(2))[4],
            13
        );
        assert_eq!(
            second.call(&mut fs, 110, walk(1, 4, &["dir", "file"]))[4],
            111
        );
        assert_eq!(
            second.call(&mut fs, 12, Body::default().u32(4).u32(2))[4],
            13
        );
        assert_eq!(first.call(&mut fs, 52, lock(3, 1, Some(0), 0, 0, 10))[7], 0);

        // Detached fids still address their original inodes in both sessions.
        fs.clear().unwrap();
        assert_eq!(&first.call(&mut fs, 116, read(3, 0, 3))[11..], b"old");
        assert_eq!(second.call(&mut fs, 118, write(4, 0, b"new"))[4], 119);
        assert_eq!(&first.call(&mut fs, 116, read(3, 0, 3))[11..], b"new");
        assert_eq!(
            second.call(&mut fs, 52, lock(4, 1, Some(0), 0, 0, 20))[7],
            1
        );
        assert_eq!(at32(&first.call(&mut fs, 40, read(2, 0, 1024)), 7), 0);
        error(
            &first.call(
                &mut fs,
                72,
                Body::default().u32(2).string("child").u32(0o755).u32(9),
            ),
            2,
        );
        error(&second.call(&mut fs, 110, walk(1, 5, &["dir"])), 2);

        // The mounted root remains usable, and replacement names have new identities.
        let replacement = fs.mkdir("dir").unwrap();
        assert_ne!(replacement, directory);
        assert_eq!(second.call(&mut fs, 110, walk(1, 5, &["dir"]))[4], 111);
        first.call(&mut fs, 120, Body::default().u32(3));
        assert!(fs.inode(file).is_some());
        second.call(&mut fs, 120, Body::default().u32(4));
        assert!(fs.inode(file).is_none());
        first.call(&mut fs, 120, Body::default().u32(2));
        assert!(fs.inode(directory).is_none());
    }

    #[test]
    fn detached_directory_fids_cannot_create_entries_or_masquerade_as_root() {
        let mut fs = filesystem();
        fs.mkdir("dir").unwrap();
        let mut c = Client::mount(&mut fs);
        c.call(&mut fs, 110, walk(1, 2, &["dir"]));
        c.call(&mut fs, 76, Body::default().u32(1).string("dir").u32(0x200));
        error(&c.call(&mut fs, 110, walk(2, 3, &[".."])), 2);
        error(
            &c.call(
                &mut fs,
                72,
                Body::default().u32(2).string("child").u32(0o755).u32(9),
            ),
            2,
        );
        let reply = c.call(&mut fs, 24, Body::default().u32(2).u64(0x7ff));
        assert_eq!(at64(&reply, 40), 0);
    }
}
