//! A synchronous 9P2000.L endpoint over a resident in-memory namespace.

use std::collections::BTreeMap;
use std::mem::take;

use crate::ninep::{
    AttributeUpdate, ByteRangeLock, ChangeSource, FileTime, Filesystem, FilesystemError,
    FilesystemIdentity, Inode, InodeId, InodeKind, LockKind, TimeUpdate,
};

#[path = "ninep_protocol/wire.rs"]
mod wire;
use wire::{Attributes, Creation, Lock, Reader, Request, Writer};

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

#[cfg(test)]
#[path = "ninep_protocol/tests.rs"]
mod tests;
