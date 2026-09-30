//! Owned filesystem handles and copied packets for the raw browser boundary.

use std::collections::BTreeMap;
use std::mem::take;
use std::str::from_utf8;

use super::{STATE, allocated_string, completion_bytes};
use crate::browser_runtime::BrowserRuntime;
use crate::ninep::{
    Change, ChangeKind, ChangeSource, FileBody, FileRead, Filesystem, FilesystemError, Inode,
    InodeId, InodeKind, Limits, LoadTicket, SeedEntry, SeedKind, SeedMetadata, SourceId,
};
use crate::ninep_backend::RustFilesystem;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Handle(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Error {
    BadHandle,
    Packet,
    Busy,
    Limit,
    Stale,
    Runtime,
    Filesystem(FilesystemError),
}

impl From<FilesystemError> for Error {
    fn from(error: FilesystemError) -> Self {
        Self::Filesystem(error)
    }
}

impl Error {
    fn status(self) -> i32 {
        -match self {
            Self::BadHandle => 9,
            Self::Packet => 22,
            Self::Busy => 16,
            Self::Limit => 28,
            Self::Stale => 116,
            Self::Runtime => 5,
            Self::Filesystem(error) => match error {
                FilesystemError::NotFound => 2,
                FilesystemError::NotDirectory => 20,
                FilesystemError::IsDirectory => 21,
                FilesystemError::AlreadyExists => 17,
                FilesystemError::NotEmpty => 39,
                FilesystemError::FileTooLarge => 27,
                FilesystemError::NoSpace => 28,
                FilesystemError::NameTooLong => 36,
                FilesystemError::InvalidPath => 22,
                FilesystemError::StaleLoad => 116,
                FilesystemError::LoadFailed | FilesystemError::NeedsLoad => 5,
            },
        }
    }
}

struct HostRead {
    inode: InodeId,
    generation: u64,
    ticket: LoadTicket,
}

// A host read owns its pin until finish or cancellation. Filesystem reset
// changes the generation, so cleanup never releases an ID in a new inode arena.
struct Entry {
    tree: RustFilesystem,
    reads: BTreeMap<u32, HostRead>,
}

/// Handles and host request IDs are never recycled during one WASM instance.
pub(super) struct FilesystemAbi {
    entries: BTreeMap<Handle, Entry>,
    bindings: BTreeMap<String, Handle>,
    next_handle: u32,
    next_read: u32,
    output: Vec<u8>,
    status: i32,
}

impl Default for FilesystemAbi {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            bindings: BTreeMap::new(),
            next_handle: 1,
            next_read: 1,
            output: Vec::new(),
            status: 0,
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    fn bytes(&mut self, length: usize) -> Result<&'a [u8], Error> {
        let end = self.position.checked_add(length).ok_or(Error::Packet)?;
        let bytes = self.bytes.get(self.position..end).ok_or(Error::Packet)?;
        self.position = end;
        Ok(bytes)
    }
    // Packets carry explicit little-endian widths. Strings and byte bodies
    // borrow the activation's copied input until parsing and dispatch finish.
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?.try_into().map_err(|_| Error::Packet)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(
            self.bytes(8)?.try_into().map_err(|_| Error::Packet)?,
        ))
    }
    fn blob(&mut self) -> Result<&'a [u8], Error> {
        let length = usize::try_from(self.u32()?).map_err(|_| Error::Packet)?;
        self.bytes(length)
    }
    fn string(&mut self) -> Result<&'a str, Error> {
        let value = from_utf8(self.blob()?).map_err(|_| Error::Packet)?;
        if value.contains('\0') {
            return Err(Error::Packet);
        }
        Ok(value)
    }
    fn done(&self) -> Result<(), Error> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err(Error::Packet)
        }
    }
}

fn string(output: &mut Vec<u8>, value: &str) -> Result<(), Error> {
    output.extend(
        u32::try_from(value.len())
            .map_err(|_| Error::Limit)?
            .to_le_bytes(),
    );
    output.extend(value.as_bytes());
    Ok(())
}

enum Operation<'a> {
    Read(&'a str),
    Write(&'a str, &'a [u8]),
    Mkdir(&'a str),
    Remove(&'a str),
    Rename(&'a str, &'a str),
    List(&'a str),
    Files,
    Stat(&'a str),
    Symlink(&'a str, &'a str),
    Readlink(&'a str),
    Link(&'a str, &'a str),
    Reset,
    Tracking(bool),
    Seed(Vec<SeedEntry>),
    Finish(u32),
    Cancel(u32),
    Retry(&'a str),
}

// Decode the closed operation set and its entire body before touching a tree.
// Seed entries own their strings because namespace installation is transactional.
impl<'a> Operation<'a> {
    fn parse(kind: u32, reader: &mut Reader<'a>) -> Result<Self, Error> {
        let operation = match kind {
            1 => Self::Read(reader.string()?),
            2 => Self::Write(reader.string()?, reader.blob()?),
            3 => Self::Mkdir(reader.string()?),
            4 => Self::Remove(reader.string()?),
            5 => Self::Rename(reader.string()?, reader.string()?),
            6 => Self::List(reader.string()?),
            7 => Self::Files,
            8 => Self::Stat(reader.string()?),
            9 => Self::Symlink(reader.string()?, reader.string()?),
            10 => Self::Readlink(reader.string()?),
            11 => Self::Link(reader.string()?, reader.string()?),
            12 => Self::Reset,
            13 => Self::Tracking(match reader.u32()? {
                0 => false,
                1 => true,
                _ => return Err(Error::Packet),
            }),
            14 => Self::Seed(seed(reader)?),
            15 => Self::Finish(reader.u32()?),
            16 => Self::Cancel(reader.u32()?),
            17 => Self::Retry(reader.string()?),
            _ => return Err(Error::Packet),
        };
        reader.done()?;
        Ok(operation)
    }
}

fn seed(reader: &mut Reader<'_>) -> Result<Vec<SeedEntry>, Error> {
    let count = reader.u32()?;
    // Every entry consumes at least twelve bytes, independent of its kind.
    if u64::from(count) * 12 > (reader.bytes.len() - reader.position) as u64 {
        return Err(Error::Packet);
    }
    let mut entries = Vec::new();
    for _ in 0..count {
        let path = reader.string()?.to_owned();
        let kind = match reader.u32()? {
            1 => SeedKind::Directory,
            2 => SeedKind::File {
                size: reader.u32()? as usize,
                source: SourceId(reader.u32()?),
            },
            3 => SeedKind::Symlink {
                target: reader.string()?.to_owned(),
            },
            4 => SeedKind::HardLink {
                target: reader.string()?.to_owned(),
            },
            _ => return Err(Error::Packet),
        };
        let mask = reader.u32()?;
        if mask & !63 != 0 {
            return Err(Error::Packet);
        }
        let metadata = SeedMetadata {
            mode: if mask & 1 != 0 {
                Some(reader.u32()?)
            } else {
                None
            },
            uid: if mask & 2 != 0 {
                Some(reader.u32()?)
            } else {
                None
            },
            gid: if mask & 4 != 0 {
                Some(reader.u32()?)
            } else {
                None
            },
            atime: if mask & 8 != 0 {
                Some(reader.u64()?)
            } else {
                None
            },
            mtime: if mask & 16 != 0 {
                Some(reader.u64()?)
            } else {
                None
            },
            ctime: if mask & 32 != 0 {
                Some(reader.u64()?)
            } else {
                None
            },
        };
        entries.push(SeedEntry {
            path,
            kind,
            metadata,
        });
    }
    Ok(entries)
}

// Output snapshots are independent of namespace storage. Numeric metadata
// keeps its full width instead of depending on JavaScript number precision.
fn kind(inode: &Inode) -> u32 {
    match inode.kind {
        InodeKind::Directory { .. } => 1,
        InodeKind::File(_) => 2,
        InodeKind::Symlink(_) => 3,
    }
}

fn stat(output: &mut Vec<u8>, inode: &Inode) -> Result<(), Error> {
    output.extend(inode.id.0.to_le_bytes());
    for value in [
        kind(inode),
        inode.mode,
        inode.uid,
        inode.gid,
        inode.version,
        inode.link_count,
    ] {
        output.extend(value.to_le_bytes());
    }
    output.extend(
        u64::try_from(inode.size())
            .map_err(|_| Error::Limit)?
            .to_le_bytes(),
    );
    for (seconds, nanos) in [
        (inode.atime, inode.atime_nanoseconds),
        (inode.mtime, inode.mtime_nanoseconds),
        (inode.ctime, inode.ctime_nanoseconds),
    ] {
        output.extend(seconds.to_le_bytes());
        output.extend(nanos.to_le_bytes());
    }
    Ok(())
}

fn ticket(output: &mut Vec<u8>, load: LoadTicket) -> Result<(), Error> {
    output.extend(load.inode.0.to_le_bytes());
    output.extend(load.generation.to_le_bytes());
    output.extend(load.id.to_le_bytes());
    output.extend(load.source.0.to_le_bytes());
    output.extend(
        u32::try_from(load.size)
            .map_err(|_| Error::Limit)?
            .to_le_bytes(),
    );
    Ok(())
}

// A completion repeats every ticket field, so a source cannot accidentally
// settle an inode reused after reset or a later retry of the same file.
fn read_ticket(reader: &mut Reader<'_>) -> Result<LoadTicket, Error> {
    Ok(LoadTicket {
        inode: InodeId(reader.u64()?),
        generation: reader.u64()?,
        id: reader.u64()?,
        source: SourceId(reader.u32()?),
        size: reader.u32()? as usize,
    })
}

fn change(output: &mut Vec<u8>, event: Change) -> Result<(), Error> {
    let code: u32 = match event.kind {
        ChangeKind::Create => 1,
        ChangeKind::Write => 2,
        ChangeKind::Remove => 3,
        ChangeKind::Rename => 4,
        ChangeKind::Metadata => 5,
        ChangeKind::Loaded => 6,
        ChangeKind::LoadError => 7,
        ChangeKind::Reset => 8,
        ChangeKind::Rescan => 9,
    };
    output.extend(code.to_le_bytes());
    output.extend(event.inode.map_or(0, |inode| inode.0).to_le_bytes());
    let (source, origin): (u32, u64) = match event.source {
        ChangeSource::Host => (0, 0),
        ChangeSource::HostOrigin(origin) => (0, origin),
        ChangeSource::Guest => (1, 0),
        ChangeSource::Loader => (2, 0),
    };
    output.extend(source.to_le_bytes());
    output.extend(origin.to_le_bytes());
    string(output, &event.path)?;
    output.extend(u32::from(event.old_path.is_some()).to_le_bytes());
    if let Some(path) = event.old_path {
        string(output, &path)?;
    }
    output.extend(
        u32::try_from(event.paths.len())
            .map_err(|_| Error::Limit)?
            .to_le_bytes(),
    );
    for path in event.paths {
        string(output, &path)?;
    }
    Ok(())
}

// One scratch result belongs to the ABI, not to a handle. The adapter copies
// it immediately; the next filesystem activation clears or replaces it.
impl FilesystemAbi {
    fn record(&mut self, result: Result<i32, Error>) -> i32 {
        self.status = result.unwrap_or_else(Error::status);
        if self.status < 0 {
            self.output.clear();
        }
        self.status
    }

    fn create(&mut self, bytes: &[u8]) -> Result<Handle, Error> {
        let mut reader = Reader::new(bytes);
        let limits = Limits {
            max_file_bytes: reader.u32()? as usize,
            max_tree_bytes: reader.u32()? as usize,
            max_inodes: reader.u32()? as usize,
            max_directory_entries: reader.u32()? as usize,
        };
        let now = reader.u64()?;
        reader.done()?;
        if limits.max_inodes == 0 || limits.max_file_bytes > limits.max_tree_bytes {
            return Err(Error::Packet);
        }
        let handle = Handle(self.next_handle);
        self.next_handle = self.next_handle.checked_add(1).ok_or(Error::Limit)?;
        let mut fs = Filesystem::new(limits, now);
        fs.set_change_tracking(false);
        self.entries.insert(
            handle,
            Entry {
                tree: RustFilesystem::new(fs),
                reads: BTreeMap::new(),
            },
        );
        Ok(handle)
    }

    // Each activation supplies epoch time and host origin. Pending reads pin
    // an inode separately from the path and from every guest protocol fid.
    fn call(&mut self, handle: Handle, bytes: &[u8]) -> Result<i32, Error> {
        let mut reader = Reader::new(bytes);
        let code = reader.u32()?;
        let now = reader.u64()?;
        let origin = reader.u64()?;
        let operation = Operation::parse(code, &mut reader)?;
        let entry = self.entries.get_mut(&handle).ok_or(Error::BadHandle)?;
        entry.tree.with_filesystem(|fs| {
            fs.set_time(now);
            fs.set_mutation_source(ChangeSource::HostOrigin(origin));
        });
        match operation {
            Operation::Read(path) => match entry.tree.with_filesystem(|fs| fs.read_file(path))? {
                FileRead::Resident(bytes) => self.output = bytes,
                FileRead::NeedsLoad { inode, .. } => {
                    if entry.reads.len() >= 1024 {
                        return Err(Error::Limit);
                    }
                    let request = self.next_read;
                    self.next_read = self.next_read.checked_add(1).ok_or(Error::Limit)?;
                    entry.tree.with_filesystem(|fs| fs.retain_fid(inode))?;
                    let load = match entry.tree.begin_load(inode) {
                        Ok(load) => load,
                        Err(error) => {
                            entry.tree.with_filesystem(|fs| fs.release_fid(inode))?;
                            return Err(error.into());
                        }
                    };
                    entry.reads.insert(
                        request,
                        HostRead {
                            inode,
                            generation: load.generation,
                            ticket: load,
                        },
                    );
                    self.output.extend(request.to_le_bytes());
                    return Ok(1);
                }
            },
            Operation::Finish(request) => return finish(entry, request, &mut self.output),
            Operation::Cancel(request) => {
                let read = entry.reads.remove(&request).ok_or(Error::BadHandle)?;
                release(entry, &read)?;
            }
            operation => entry
                .tree
                .with_filesystem(|fs| execute(fs, operation, &mut self.output))?,
        }
        Ok(0)
    }
}

fn release(entry: &Entry, read: &HostRead) -> Result<(), Error> {
    entry.tree.with_filesystem(|fs| {
        if read.generation == fs.generation() {
            fs.release_fid(read.inode)?;
        }
        Ok(())
    })
}

fn finish(entry: &mut Entry, request: u32, output: &mut Vec<u8>) -> Result<i32, Error> {
    let read = entry.reads.get(&request).ok_or(Error::BadHandle)?;
    let result = entry.tree.with_filesystem(|fs| {
        if fs.generation() != read.generation {
            return Err(Error::Stale);
        }
        match fs.file_body(read.inode)? {
            FileBody::Resident(bytes) => {
                output.extend(bytes);
                Ok(0)
            }
            FileBody::Loading { load, .. } if *load == read.ticket.id => {
                output.extend(request.to_le_bytes());
                Ok(1)
            }
            _ => Err(Error::Filesystem(FilesystemError::LoadFailed)),
        }
    });
    if result != Ok(1) {
        let read = entry.reads.remove(&request).ok_or(Error::BadHandle)?;
        release(entry, &read)?;
    }
    result
}

// Whole-file operations retain namespace validation and quotas. A source
// supplies seed bodies only; host writes remain authoritative in memory.
fn execute(
    fs: &mut Filesystem,
    operation: Operation<'_>,
    output: &mut Vec<u8>,
) -> Result<(), Error> {
    match operation {
        Operation::Write(path, bytes) => {
            fs.write_file(path, bytes)?;
        }
        Operation::Mkdir(path) => {
            fs.mkdir(path)?;
        }
        Operation::Remove(path) => fs.remove(path)?,
        Operation::Rename(old, new) => fs.rename(old, new)?,
        Operation::Symlink(path, target) => {
            fs.symlink(path, target)?;
        }
        Operation::Readlink(path) => output.extend(fs.readlink(path)?.as_bytes()),
        Operation::Link(existing, new) => fs.hard_link(existing, new)?,
        Operation::Reset => fs.reset()?,
        Operation::Tracking(enabled) => fs.set_change_tracking(enabled),
        Operation::Seed(entries) => fs.install_seed(&entries)?,
        Operation::Retry(path) => fs.retry_load(path)?,
        Operation::Stat(path) => stat(
            output,
            fs.inode(fs.lookup(path)?)
                .ok_or(FilesystemError::NotFound)?,
        )?,
        Operation::Files => {
            let files = fs.list_files();
            output.extend(
                u32::try_from(files.len())
                    .map_err(|_| Error::Limit)?
                    .to_le_bytes(),
            );
            for path in files {
                string(output, &path)?;
            }
        }
        Operation::List(path) => {
            let entries = fs.list_directory(path)?;
            output.extend(
                u32::try_from(entries.len())
                    .map_err(|_| Error::Limit)?
                    .to_le_bytes(),
            );
            for entry in entries {
                string(output, &entry.name)?;
                output.extend(entry.inode.0.to_le_bytes());
                output.extend(
                    kind(fs.inode(entry.inode).ok_or(FilesystemError::NotFound)?).to_le_bytes(),
                );
                output.extend(entry.cookie.to_le_bytes());
            }
        }
        Operation::Read(_) | Operation::Finish(_) | Operation::Cancel(_) => {
            return Err(Error::Packet);
        }
    }
    Ok(())
}

// Input addresses must name adapter-owned allocations. Copying them before
// dispatch keeps views and host callbacks outside borrowed runtime state.
#[must_use]
pub extern "C" fn riscbox_fs_create(address: u32, length: u32) -> u32 {
    STATE.with_borrow_mut(|state| {
        state.filesystems.output.clear();
        let result = completion_bytes(state, address, length)
            .ok_or(Error::Packet)
            .and_then(|bytes| state.filesystems.create(&bytes));
        match result {
            Ok(handle) => {
                state.filesystems.record(Ok(0));
                handle.0
            }
            Err(error) => {
                state.filesystems.record(Err(error));
                0
            }
        }
    })
}

// Bindings resolve the configuration's server keys before VM construction.
// Multiple keys may alias one tree, but its attachment guard admits one VM.
#[must_use]
pub extern "C" fn riscbox_fs_bind(handle: u32, address: u32, length: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        state.filesystems.output.clear();
        let result = (|| {
            let key = allocated_string(state, address, length)
                .filter(|key| !key.is_empty() && !key.contains('\0'))
                .ok_or(Error::Packet)?;
            let tree = state
                .filesystems
                .entries
                .get(&Handle(handle))
                .ok_or(Error::BadHandle)?
                .tree
                .clone();
            // Rebinding is allowed only before startup, so image selection can
            // reuse fixed configuration keys without replacing namespace handles.
            if state.filesystems.bindings.contains_key(&key) {
                state
                    .runtime
                    .unregister_filesystem(&key)
                    .map_err(|_| Error::Busy)?;
            }
            state
                .runtime
                .register_filesystem_handle(key.clone(), tree)
                .map_err(|_| Error::Busy)?;
            state.filesystems.bindings.insert(key, Handle(handle));
            Ok(0)
        })();
        state.filesystems.record(result)
    })
}

// Poll after the namespace borrow ends, including errors that settle a load
// as failed. Active guest descriptors then receive ordinary tagged errors.
#[must_use]
pub extern "C" fn riscbox_fs_call(handle: u32, address: u32, length: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        state.filesystems.output.clear();
        let result = completion_bytes(state, address, length)
            .ok_or(Error::Packet)
            .and_then(|bytes| state.filesystems.call(Handle(handle), &bytes));
        let result = match state.runtime.poll_filesystems() {
            Ok(()) => result,
            Err(_) => Err(Error::Runtime),
        };
        state.filesystems.record(result)
    })
}

// Closing removes inactive bindings and host pins. VM-owned trees cannot be
// closed while their endpoints still need source dispatch or completion.
#[must_use]
pub extern "C" fn riscbox_fs_close(handle: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        state.filesystems.output.clear();
        let result = close(&mut state.filesystems, &mut state.runtime, Handle(handle));
        state.filesystems.record(result)
    })
}

fn close(
    abi: &mut FilesystemAbi,
    runtime: &mut BrowserRuntime,
    handle: Handle,
) -> Result<i32, Error> {
    let entry = abi.entries.get(&handle).ok_or(Error::BadHandle)?;
    if entry.tree.is_attached() {
        return Err(Error::Busy);
    }
    let keys: Vec<String> = abi
        .bindings
        .iter()
        .filter(|(_, bound)| **bound == handle)
        .map(|(key, _)| key.clone())
        .collect();
    for key in &keys {
        runtime
            .unregister_filesystem(key)
            .map_err(|_| Error::Busy)?;
    }
    for key in keys {
        abi.bindings.remove(&key);
    }
    let mut entry = abi.entries.remove(&handle).ok_or(Error::BadHandle)?;
    for (_, read) in take(&mut entry.reads) {
        release(&entry, &read)?;
    }
    Ok(0)
}

// Only started loads enter this queue. Tickets are copied before a plugin
// promise begins; reset and host writes filter superseded undispatched work.
#[must_use]
pub extern "C" fn riscbox_fs_next_load() -> u32 {
    STATE.with_borrow_mut(|state| {
        state.filesystems.output.clear();
        let work = state
            .filesystems
            .entries
            .iter()
            .find_map(|(handle, entry)| entry.tree.next_load().map(|load| (*handle, load)));
        if let Some((handle, load)) = work {
            let result = ticket(&mut state.filesystems.output, load).map(|()| 0);
            if state.filesystems.record(result) == 0 {
                return handle.0;
            }
        } else {
            state.filesystems.record(Ok(0));
        }
        0
    })
}

// Late tickets are acknowledged as ignored. Failed lengths settle the body
// as failed and still poll sessions before returning the source error.
#[must_use]
pub extern "C" fn riscbox_fs_complete_load(handle: u32, address: u32, length: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        state.filesystems.output.clear();
        let result = (|| {
            let bytes = completion_bytes(state, address, length).ok_or(Error::Packet)?;
            let mut reader = Reader::new(&bytes);
            let load = read_ticket(&mut reader)?;
            let now = reader.u64()?;
            let success = match reader.u32()? {
                0 => true,
                1 => false,
                _ => return Err(Error::Packet),
            };
            let data = reader.blob()?;
            reader.done()?;
            if !success && !data.is_empty() {
                return Err(Error::Packet);
            }
            let entry = state
                .filesystems
                .entries
                .get(&Handle(handle))
                .ok_or(Error::BadHandle)?;
            let result = entry.tree.with_filesystem(|fs| {
                fs.set_time(now);
                if success {
                    fs.complete_load(load, data.to_vec())
                } else {
                    fs.fail_load(load)
                }
            });
            match result {
                Ok(()) => Ok(0),
                Err(FilesystemError::StaleLoad) => Ok(1),
                Err(error) => Err(error.into()),
            }
        })();
        let result = match state.runtime.poll_filesystems() {
            Ok(()) => result,
            Err(_) => Err(Error::Runtime),
        };
        state.filesystems.record(result)
    })
}

// Events include aliases and rename prefixes; overflow and reset are explicit
// invalidations. Listener callbacks run after copying this snapshot.
#[must_use]
pub extern "C" fn riscbox_fs_next_change(handle: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        state.filesystems.output.clear();
        let result = (|| {
            let entry = state
                .filesystems
                .entries
                .get(&Handle(handle))
                .ok_or(Error::BadHandle)?;
            let Some(event) = entry.tree.with_filesystem(Filesystem::next_change) else {
                return Ok(0);
            };
            change(&mut state.filesystems.output, event)?;
            Ok(1)
        })();
        state.filesystems.record(result)
    })
}

#[must_use]
pub extern "C" fn riscbox_fs_status() -> i32 {
    STATE.with_borrow(|state| state.filesystems.status)
}

#[must_use]
pub extern "C" fn riscbox_fs_data_address() -> u32 {
    STATE
        .with_borrow(|state| u32::try_from(state.filesystems.output.as_ptr() as usize).unwrap_or(0))
}

#[must_use]
pub extern "C" fn riscbox_fs_data_length() -> u32 {
    STATE.with_borrow(|state| u32::try_from(state.filesystems.output.len()).unwrap_or(0))
}

#[cfg(test)]
#[path = "ninep/tests.rs"]
mod tests;
