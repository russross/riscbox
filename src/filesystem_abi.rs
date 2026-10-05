//! Owned filesystem handles and copied packets for the raw browser boundary.

use std::collections::BTreeMap;
use std::str::from_utf8;

use crate::browser_abi::{STATE, allocated_string, completion_bytes};
use crate::ninep::{
    AttributeUpdate, Change, ChangeKind, ChangeSource, FileTime, Filesystem, FilesystemError,
    Inode, InodeKind, SharedFilesystem, TimeUpdate,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Handle(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Error {
    BadHandle,
    Packet,
    Busy,
    Limit,
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
            },
        }
    }
}

// Handles reference VM-owned namespaces and are invalidated on destruction.
struct Entry {
    tree: SharedFilesystem,
}

/// Handles are never recycled during one WASM instance.
pub(crate) struct FilesystemAbi {
    entries: BTreeMap<Handle, Entry>,
    names: BTreeMap<String, Handle>,
    next_handle: u32,
    output: Vec<u8>,
    status: i32,
}

impl Default for FilesystemAbi {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            names: BTreeMap::new(),
            next_handle: 1,
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

#[derive(Clone, Copy)]
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
    Clear,
    Tracking(bool),
    Attributes(&'a str, AttributeUpdate),
}

// Decode the closed operation set and its entire body before touching a tree.
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
            15 => Self::Clear,
            14 => {
                let path = reader.string()?;
                let mode = reader.u32()?;
                if mode > 0o7777 {
                    return Err(Error::Packet);
                }
                let uid = reader.u32()?;
                let gid = reader.u32()?;
                // Validate both times before dispatch so malformed packets cannot
                // partially update permissions or ownership.
                let atime = FileTime::new(reader.u64()?, u64::from(reader.u32()?))?;
                let mtime = FileTime::new(reader.u64()?, u64::from(reader.u32()?))?;
                Self::Attributes(
                    path,
                    AttributeUpdate {
                        mode: Some(mode),
                        uid: Some(uid),
                        gid: Some(gid),
                        atime: Some(TimeUpdate::Explicit(atime)),
                        mtime: Some(TimeUpdate::Explicit(mtime)),
                        ..AttributeUpdate::default()
                    },
                )
            }
            _ => return Err(Error::Packet),
        };
        reader.done()?;
        Ok(operation)
    }
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

fn change(output: &mut Vec<u8>, event: Change) -> Result<(), Error> {
    let code: u32 = match event.kind {
        ChangeKind::Create => 1,
        ChangeKind::Write => 2,
        ChangeKind::Remove => 3,
        ChangeKind::Rename => 4,
        ChangeKind::Metadata => 5,
        ChangeKind::Reset => 8,
        ChangeKind::Rescan => 9,
    };
    output.extend(code.to_le_bytes());
    output.extend(event.inode.map_or(0, |inode| inode.0).to_le_bytes());
    let (source, origin): (u32, u64) = match event.source {
        ChangeSource::Host => (0, 0),
        ChangeSource::HostOrigin(origin) => (0, origin),
        ChangeSource::Guest => (1, 0),
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
    pub(crate) fn destroy(&mut self) {
        self.entries.clear();
        self.names.clear();
        self.output.clear();
    }
    fn record(&mut self, result: Result<i32, Error>) -> i32 {
        self.status = result.unwrap_or_else(Error::status);
        if self.status < 0 {
            self.output.clear();
        }
        self.status
    }

    // Each activation supplies epoch time and host origin, and finishes all
    // namespace access before returning copied bytes to the host.
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
            Operation::Read(path) => {
                self.output = entry.tree.with_filesystem(|fs| fs.read_file(path))?;
            }
            operation => entry
                .tree
                .with_filesystem(|fs| execute(fs, operation, &mut self.output))?,
        }
        Ok(0)
    }
}

// Whole-file operations preserve namespace validation and quota accounting.
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
        Operation::Clear => fs.clear()?,
        Operation::Tracking(enabled) => fs.set_change_tracking(enabled),
        Operation::Stat(path) => stat(
            output,
            fs.inode(fs.lookup(path)?)
                .ok_or(FilesystemError::NotFound)?,
        )?,
        Operation::Attributes(path, attributes) => {
            fs.update_attributes(fs.lookup(path)?, attributes)?;
        }
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
        Operation::Read(_) => {
            return Err(Error::Packet);
        }
    }
    Ok(())
}

// Each resident filesystem operation finishes before its namespace borrow ends.
#[must_use]
pub extern "C" fn riscbox_fs_call(handle: u32, address: u32, length: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        state.filesystems.output.clear();
        let result = completion_bytes(state, address, length)
            .ok_or(Error::Packet)
            .and_then(|bytes| {
                if bytes.get(..4) == Some(&12_u32.to_le_bytes()) && !state.runtime.is_halted() {
                    return Err(Error::Busy);
                }
                state.filesystems.call(Handle(handle), &bytes)
            });
        state.filesystems.record(result)
    })
}

// Configuration creates named shares; lookup returns a VM-scoped handle.
#[must_use]
pub extern "C" fn riscbox_fs_get(address: u32, length: u32) -> u32 {
    STATE.with_borrow_mut(|state| {
        let result = (|| {
            let name = allocated_string(state, address, length).ok_or(Error::Packet)?;
            if let Some(handle) = state.filesystems.names.get(&name) {
                return Ok(*handle);
            }
            let tree = state
                .runtime
                .filesystem_handle(&name)
                .ok_or(Error::BadHandle)?;
            let handle = Handle(state.filesystems.next_handle);
            state.filesystems.next_handle = handle.0.checked_add(1).ok_or(Error::Limit)?;
            state.filesystems.entries.insert(handle, Entry { tree });
            state.filesystems.names.insert(name, handle);
            Ok(handle)
        })();
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
mod tests {
    use super::{Entry, Error, FilesystemAbi, Handle};
    use crate::ninep::SharedFilesystem;
    use crate::ninep::{Filesystem, FilesystemError, InodeKind, Limits};

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

    fn invoke(
        abi: &mut FilesystemAbi,
        handle: Handle,
        code: u32,
        body: &[u8],
    ) -> Result<i32, Error> {
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
                tree: SharedFilesystem::new(Filesystem::new(
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
                tree: SharedFilesystem::new(Filesystem::new(Limits::default(), 100)),
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

    // Attribute packets validate completely before changing a live inode.
    #[test]
    fn restored_attributes_keep_permissions_ownership_and_timestamp_precision() {
        let mut abi = FilesystemAbi::default();
        let handle = Handle(1);
        abi.entries.insert(
            handle,
            Entry {
                tree: SharedFilesystem::new(Filesystem::new(Limits::default(), 100)),
            },
        );
        write(&mut abi, handle, "file", "bytes");
        let mut body = path("file");
        for value in [0o751_u32, 123, 456] {
            body.extend(value.to_le_bytes());
        }
        body.extend(789_u64.to_le_bytes());
        body.extend(123_456_789_u32.to_le_bytes());
        body.extend(900_u64.to_le_bytes());
        body.extend(987_654_321_u32.to_le_bytes());
        assert_eq!(invoke(&mut abi, handle, 14, &body), Ok(0));
        let before = abi.entries[&handle]
            .tree
            .with_filesystem(|fs| fs.inode(fs.lookup("file").unwrap()).unwrap().clone());
        assert_eq!((before.mode, before.uid, before.gid), (0o751, 123, 456));
        assert_eq!((before.atime, before.atime_nanoseconds), (789, 123_456_789));
        assert_eq!((before.mtime, before.mtime_nanoseconds), (900, 987_654_321));

        for end in 0..body.len() {
            assert_eq!(
                invoke(&mut abi, handle, 14, &body[..end]),
                Err(Error::Packet)
            );
        }
        let end = body.len();
        body[end - 4..].copy_from_slice(&1_000_000_000_u32.to_le_bytes());
        assert_eq!(
            invoke(&mut abi, handle, 14, &body),
            Err(Error::Filesystem(FilesystemError::InvalidPath))
        );
        abi.entries[&handle].tree.with_filesystem(|fs| {
            assert_eq!(fs.inode(fs.lookup("file").unwrap()).unwrap(), &before);
        });
    }
}
