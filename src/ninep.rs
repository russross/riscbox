//! Shared 9p namespace state, independent of the lifetime of a guest VM.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

/// Shared namespace ownership stays separate from device and VM lifetimes.
#[derive(Clone)]
pub struct SharedFilesystem {
    filesystem: Rc<RefCell<Filesystem>>,
}

impl SharedFilesystem {
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

const MAX_NAME_BYTES: usize = 255;

/// A lifetime token distinguishes independently created namespaces.
#[derive(Clone, Debug)]
pub struct FilesystemIdentity(Rc<()>);

impl PartialEq for FilesystemIdentity {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for FilesystemIdentity {}

/// Protocol timestamps retain nanoseconds independently of the runtime clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileTime {
    seconds: u64,
    nanoseconds: u32,
}

impl FileTime {
    /// # Errors
    /// Rejects nanoseconds outside one second.
    pub fn new(seconds: u64, nanoseconds: u64) -> Result<Self, FilesystemError> {
        if nanoseconds >= 1_000_000_000 {
            return Err(FilesystemError::InvalidPath);
        }
        Ok(Self {
            seconds,
            nanoseconds: u32::try_from(nanoseconds).map_err(|_| FilesystemError::InvalidPath)?,
        })
    }

    #[must_use]
    pub const fn seconds(self) -> u64 {
        self.seconds
    }

    #[must_use]
    pub const fn nanoseconds(self) -> u32 {
        self.nanoseconds
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeUpdate {
    Current,
    Explicit(FileTime),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AttributeUpdate {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub size: Option<u64>,
    pub atime: Option<TimeUpdate>,
    pub mtime: Option<TimeUpdate>,
    pub change_ctime: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpaceUsage {
    pub maximum_bytes: usize,
    pub used_bytes: usize,
    pub maximum_inodes: usize,
    pub used_inodes: usize,
}

/// A stable identity for one inode, including one with no remaining links.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InodeId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InodeKind {
    Directory {
        entries: BTreeMap<String, DirectoryEntry>,
        next_cookie: u64,
    },
    File(Vec<u8>),
    Symlink(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryEntry {
    pub inode: InodeId,
    pub cookie: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListedEntry {
    pub name: String,
    pub inode: InodeId,
    pub cookie: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Inode {
    pub id: InodeId,
    pub kind: InodeKind,
    pub parent: Option<InodeId>,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub version: u32,
    pub link_count: u32,
    pub fid_refs: u32,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub atime_nanoseconds: u32,
    pub mtime_nanoseconds: u32,
    pub ctime_nanoseconds: u32,
}

impl Inode {
    #[must_use]
    pub fn size(&self) -> usize {
        match &self.kind {
            InodeKind::File(body) => body.len(),
            InodeKind::Symlink(target) => target.len(),
            InodeKind::Directory { .. } => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockKind {
    Read,
    Write,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByteRangeLock {
    pub inode: InodeId,
    pub session: u64,
    pub kind: LockKind,
    pub start: u64,
    pub length: u64,
    pub process: u32,
    pub client: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_file_bytes: usize,
    pub max_tree_bytes: usize,
    pub max_inodes: usize,
    pub max_directory_entries: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Metadata {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub atime: Option<u64>,
    pub mtime: Option<u64>,
    pub ctime: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_bytes: 256 * 1024 * 1024,
            max_tree_bytes: 1024 * 1024 * 1024,
            max_inodes: 1 << 20,
            max_directory_entries: 1 << 20,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilesystemError {
    InvalidPath,
    NotFound,
    NotDirectory,
    IsDirectory,
    AlreadyExists,
    NotEmpty,
    FileTooLarge,
    NoSpace,
    NameTooLong,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeKind {
    Create,
    Write,
    Metadata,
    Remove,
    Rename,
    Reset,
    /// Detailed records were dropped; subscribers must refresh their view.
    Rescan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeSource {
    Host,
    HostOrigin(u64),
    Guest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Change {
    pub kind: ChangeKind,
    pub inode: Option<InodeId>,
    pub path: String,
    pub old_path: Option<String>,
    pub paths: Vec<String>,
    pub source: ChangeSource,
}

/// The tree remains live across VM reset and shutdown.
pub struct Filesystem {
    identity: FilesystemIdentity,
    next_session: u64,
    inodes: BTreeMap<InodeId, Inode>,
    links: BTreeMap<InodeId, BTreeSet<(InodeId, String)>>,
    root: InodeId,
    next_inode: u64,
    generation: u64,
    limits: Limits,
    logical_bytes: usize,
    directory_entries: usize,
    locks: Vec<ByteRangeLock>,
    changes: VecDeque<Change>,
    now: u64,
    mutation_source: ChangeSource,
    record_changes: bool,
}

impl Filesystem {
    /// Creates a standalone namespace at the supplied epoch time in seconds.
    #[must_use]
    pub fn new(limits: Limits, now: u64) -> Self {
        let root = InodeId(1);
        let mut inodes = BTreeMap::new();
        inodes.insert(
            root,
            Inode {
                id: root,
                kind: InodeKind::Directory {
                    entries: BTreeMap::new(),
                    next_cookie: 1,
                },
                parent: None,
                mode: 0o755,
                uid: 1000,
                gid: 1000,
                version: 0,
                link_count: 1,
                fid_refs: 0,
                atime: now,
                mtime: now,
                ctime: now,
                atime_nanoseconds: 0,
                mtime_nanoseconds: 0,
                ctime_nanoseconds: 0,
            },
        );
        Self {
            identity: FilesystemIdentity(Rc::new(())),
            next_session: 1,
            inodes,
            links: BTreeMap::new(),
            root,
            next_inode: 2,
            generation: 1,
            limits,
            logical_bytes: 0,
            directory_entries: 0,
            locks: Vec::new(),
            changes: VecDeque::new(),
            now,
            mutation_source: ChangeSource::Host,
            record_changes: true,
        }
    }

    /// Updates the epoch time supplied by the runtime before a batch of work.
    /// Raw WASM does not have an implicit operating-system clock.
    pub const fn set_time(&mut self, now: u64) {
        self.now = now;
    }

    /// Selects the origin recorded for subsequent mutations in this activation.
    /// The runtime sets this before guest work or a host operation; no callback
    /// or asynchronous continuation runs while the namespace is borrowed.
    pub const fn set_mutation_source(&mut self, source: ChangeSource) {
        self.mutation_source = source;
    }

    fn touch(inode: &mut Inode, now: u64) {
        inode.version = inode.version.wrapping_add(1);
        inode.mtime = now;
        inode.ctime = now;
        inode.mtime_nanoseconds = 0;
        inode.ctime_nanoseconds = 0;
    }

    #[must_use]
    pub fn identity(&self) -> FilesystemIdentity {
        self.identity.clone()
    }

    /// # Errors
    /// Reports exhaustion instead of reusing a live session's cleanup ID.
    pub fn allocate_session(&mut self) -> Result<u64, FilesystemError> {
        let id = self.next_session;
        self.next_session = id.checked_add(1).ok_or(FilesystemError::NoSpace)?;
        Ok(id)
    }

    #[must_use]
    pub const fn time(&self) -> u64 {
        self.now
    }

    #[must_use]
    pub const fn mutation_source(&self) -> ChangeSource {
        self.mutation_source
    }

    #[must_use]
    pub fn space_usage(&self) -> SpaceUsage {
        SpaceUsage {
            maximum_bytes: self.limits.max_tree_bytes,
            used_bytes: self.logical_bytes,
            maximum_inodes: self.limits.max_inodes,
            used_inodes: self.inodes.len(),
        }
    }

    #[must_use]
    pub const fn root(&self) -> InodeId {
        self.root
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn inode(&self, id: InodeId) -> Option<&Inode> {
        self.inodes.get(&id)
    }

    /// Retains an inode while a protocol fid refers to it.
    ///
    /// # Errors
    /// Returns an unknown-inode or reference-count exhaustion error.
    pub fn retain_fid(&mut self, id: InodeId) -> Result<(), FilesystemError> {
        let inode = self.inodes.get_mut(&id).ok_or(FilesystemError::NotFound)?;
        inode.fid_refs = inode
            .fid_refs
            .checked_add(1)
            .ok_or(FilesystemError::NoSpace)?;
        Ok(())
    }

    /// Releases an inode and reclaims a file after its last link and fid vanish.
    ///
    /// # Errors
    /// Returns an unknown-inode or invalid-reference error.
    pub fn release_fid(&mut self, id: InodeId) -> Result<(), FilesystemError> {
        let inode = self.inodes.get_mut(&id).ok_or(FilesystemError::NotFound)?;
        inode.fid_refs = inode
            .fid_refs
            .checked_sub(1)
            .ok_or(FilesystemError::InvalidPath)?;
        if inode.fid_refs == 0 && inode.link_count == 0 {
            let old_size = match &inode.kind {
                InodeKind::File(body) => body.len(),
                _ => 0,
            };
            self.inodes.remove(&id);
            self.links.remove(&id);
            self.logical_bytes -= old_size;
            self.locks.retain(|lock| lock.inode != id);
        }
        Ok(())
    }

    pub fn next_change(&mut self) -> Option<Change> {
        self.changes.pop_front()
    }

    /// Disables event work when the host has no subscribers. Enabling it
    /// invalidates the previous host view because intervening edits were lost.
    pub fn set_change_tracking(&mut self, enabled: bool) {
        if self.record_changes == enabled {
            return;
        }
        self.record_changes = enabled;
        self.changes.clear();
        if enabled {
            self.emit(ChangeKind::Rescan, None, "", None, self.mutation_source);
        }
    }

    /// Finds every current path of an inode, including all hard links.
    #[must_use]
    pub fn paths_of(&self, target: InodeId) -> Vec<String> {
        if target == self.root {
            return vec![String::new()];
        }
        let mut result = Vec::new();
        // Reverse links visit only the inode's aliases and their ancestors.
        // A directory has one parent, and rename rejects directory cycles.
        let mut ancestors = vec![(target, String::new())];
        while let Some((id, suffix)) = ancestors.pop() {
            if id == self.root {
                result.push(suffix);
                continue;
            }
            if let Some(links) = self.links.get(&id) {
                for (parent, name) in links {
                    let path = if suffix.is_empty() {
                        name.clone()
                    } else {
                        format!("{name}/{suffix}")
                    };
                    ancestors.push((*parent, path));
                }
            }
        }
        result.sort();
        result
    }

    fn emit(
        &mut self,
        kind: ChangeKind,
        inode: Option<InodeId>,
        path: &str,
        old_path: Option<&str>,
        source: ChangeSource,
    ) {
        if !self.record_changes {
            return;
        }
        // A stalled consumer cannot accumulate an unbounded mutation history.
        // Rescan is an invalidation notice, not a filesystem or protocol reset.
        if self
            .changes
            .front()
            .is_some_and(|change| change.kind == ChangeKind::Rescan)
        {
            return;
        }
        if self.changes.len() >= 1024 {
            self.changes.clear();
            self.changes.push_back(Change {
                kind: ChangeKind::Rescan,
                inode: None,
                path: String::new(),
                old_path: None,
                paths: Vec::new(),
                source,
            });
            return;
        }
        let path = path.trim_start_matches('/');
        let old_path = old_path.map(|path| path.trim_start_matches('/'));
        let mut paths = inode.map_or_else(Vec::new, |id| self.paths_of(id));
        if !path.is_empty() && !paths.iter().any(|current| current == path) {
            paths.push(path.to_owned());
            paths.sort();
        }
        self.changes.push_back(Change {
            kind,
            inode,
            path: path.to_owned(),
            old_path: old_path.map(str::to_owned),
            paths,
            source,
        });
    }

    fn emit_inode(&mut self, kind: ChangeKind, inode: InodeId, source: ChangeSource) {
        self.emit(kind, Some(inode), "", None, source);
        if let Some(change) = self.changes.back_mut()
            && change.inode == Some(inode)
        {
            change.path = change.paths.first().cloned().unwrap_or_default();
        }
    }

    fn ranges_overlap(
        left_start: u64,
        left_length: u64,
        right_start: u64,
        right_length: u64,
    ) -> bool {
        let left_end = if left_length == 0 {
            u128::MAX
        } else {
            u128::from(left_start) + u128::from(left_length)
        };
        let right_end = if right_length == 0 {
            u128::MAX
        } else {
            u128::from(right_start) + u128::from(right_length)
        };
        u128::from(left_start) < right_end && u128::from(right_start) < left_end
    }

    fn check_file_inode(&self, id: InodeId) -> Result<(), FilesystemError> {
        let inode = self.inodes.get(&id).ok_or(FilesystemError::NotFound)?;
        if matches!(inode.kind, InodeKind::File(_)) {
            Ok(())
        } else {
            Err(FilesystemError::IsDirectory)
        }
    }

    /// Returns the first incompatible lock held by another process/client.
    ///
    /// # Errors
    /// Returns an inode or type error when the target is not a regular file.
    pub fn conflicting_lock(
        &self,
        requested: &ByteRangeLock,
    ) -> Result<Option<&ByteRangeLock>, FilesystemError> {
        self.check_file_inode(requested.inode)?;
        Ok(self.locks.iter().find(|held| {
            held.inode == requested.inode
                && !Self::same_lock_owner(held, requested)
                && (held.kind == LockKind::Write || requested.kind == LockKind::Write)
                && Self::ranges_overlap(held.start, held.length, requested.start, requested.length)
        }))
    }

    /// Converts the owner's range after checking other owners for conflicts.
    ///
    /// # Errors
    /// Returns an inode or type error; a conflict is reported as `Ok(false)`.
    pub fn try_lock(&mut self, requested: ByteRangeLock) -> Result<bool, FilesystemError> {
        if self.conflicting_lock(&requested)?.is_some() {
            return Ok(false);
        }
        self.subtract_owned_range(&requested);
        self.locks.push(requested);
        Ok(true)
    }

    fn same_lock_owner(left: &ByteRangeLock, right: &ByteRangeLock) -> bool {
        left.process == right.process && left.client == right.client
    }

    fn lock_end(lock: &ByteRangeLock) -> u128 {
        if lock.length == 0 {
            u128::from(u64::MAX) + 1
        } else {
            (u128::from(lock.start) + u128::from(lock.length)).min(u128::from(u64::MAX) + 1)
        }
    }

    // Replacement and unlock keep every byte outside the requested interval.
    // Zero length extends to the end of the representable file-offset space.
    fn subtract_owned_range(&mut self, requested: &ByteRangeLock) {
        let mut retained = Vec::with_capacity(self.locks.len());
        for held in self.locks.drain(..) {
            if held.inode != requested.inode
                || !Self::same_lock_owner(&held, requested)
                || !Self::ranges_overlap(held.start, held.length, requested.start, requested.length)
            {
                retained.push(held);
                continue;
            }
            let held_end = Self::lock_end(&held);
            let requested_end = Self::lock_end(requested);
            if held.start < requested.start {
                let mut left = held.clone();
                left.length = requested.start - held.start;
                retained.push(left);
            }
            if requested_end < held_end {
                let mut right = held;
                right.start =
                    u64::try_from(requested_end).expect("remaining interval starts in u64");
                right.length = if held_end == u128::from(u64::MAX) + 1 {
                    0
                } else {
                    u64::try_from(held_end - requested_end).expect("remaining interval fits u64")
                };
                retained.push(right);
            }
        }
        self.locks = retained;
    }

    /// Removes only the requested bytes owned by one process and client.
    ///
    /// # Errors
    /// Returns an inode or type error when the target is not a regular file.
    pub fn unlock(
        &mut self,
        inode: InodeId,
        session: u64,
        process: u32,
        client: &str,
        start: u64,
        length: u64,
    ) -> Result<(), FilesystemError> {
        self.check_file_inode(inode)?;
        self.subtract_owned_range(&ByteRangeLock {
            inode,
            session,
            process,
            client: client.to_owned(),
            kind: LockKind::Read,
            start,
            length,
        });
        Ok(())
    }

    pub fn release_session_locks(&mut self, session: u64) {
        self.locks.retain(|held| held.session != session);
    }

    fn parts(path: &str) -> Result<Vec<&str>, FilesystemError> {
        if path.contains('\0') {
            return Err(FilesystemError::InvalidPath);
        }
        let relative = path.strip_prefix('/').unwrap_or(path);
        if relative.is_empty() {
            return Ok(Vec::new());
        }
        let parts: Vec<_> = relative.split('/').collect();
        if parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
        {
            return Err(FilesystemError::InvalidPath);
        }
        if parts.iter().any(|part| part.len() > MAX_NAME_BYTES) {
            return Err(FilesystemError::NameTooLong);
        }
        Ok(parts)
    }

    /// Finds an inode by a validated relative or root-based path.
    ///
    /// # Errors
    /// Returns a path or lookup error if the path cannot identify an inode.
    pub fn lookup(&self, path: &str) -> Result<InodeId, FilesystemError> {
        let mut current = self.root;
        for part in Self::parts(path)? {
            let Some(inode) = self.inodes.get(&current) else {
                return Err(FilesystemError::NotFound);
            };
            let InodeKind::Directory { entries, .. } = &inode.kind else {
                return Err(FilesystemError::NotDirectory);
            };
            current = entries.get(part).ok_or(FilesystemError::NotFound)?.inode;
        }
        Ok(current)
    }

    /// Resolves one literal child name without following a symbolic link.
    ///
    /// # Errors
    /// Returns a type or missing-entry error.
    pub fn child(&self, parent: InodeId, name: &str) -> Result<InodeId, FilesystemError> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
            return Err(FilesystemError::InvalidPath);
        }
        if name.len() > MAX_NAME_BYTES {
            return Err(FilesystemError::NameTooLong);
        }
        let inode = self.inodes.get(&parent).ok_or(FilesystemError::NotFound)?;
        let InodeKind::Directory { entries, .. } = &inode.kind else {
            return Err(FilesystemError::NotDirectory);
        };
        Ok(entries.get(name).ok_or(FilesystemError::NotFound)?.inode)
    }

    #[must_use]
    pub fn parent_inode(&self, id: InodeId) -> Option<InodeId> {
        self.inodes.get(&id).and_then(|inode| inode.parent)
    }

    /// Lists immediate children in stable directory-cookie order.
    ///
    /// # Errors
    /// Returns a path or type error if the path does not name a directory.
    pub fn list_directory(&self, path: &str) -> Result<Vec<ListedEntry>, FilesystemError> {
        let id = self.lookup(path)?;
        self.directory_entries(id)
    }

    /// Lists regular-file paths recursively, without loading their bodies.
    #[must_use]
    pub fn list_files(&self) -> Vec<String> {
        let mut paths = Vec::new();
        let mut directories = vec![(self.root, String::new())];
        while let Some((directory, prefix)) = directories.pop() {
            let Some(Inode {
                kind: InodeKind::Directory { entries, .. },
                ..
            }) = self.inodes.get(&directory)
            else {
                continue;
            };
            for (name, entry) in entries {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                match self.inodes.get(&entry.inode).map(|inode| &inode.kind) {
                    Some(InodeKind::File(_)) => paths.push(path),
                    Some(InodeKind::Directory { .. }) => directories.push((entry.inode, path)),
                    _ => {}
                }
            }
        }
        paths.sort();
        paths
    }

    /// Updates an inode's metadata and advances its QID version.
    ///
    /// # Errors
    /// Returns an unknown-inode error if it has been retired.
    pub fn set_metadata(&mut self, id: InodeId, update: Metadata) -> Result<(), FilesystemError> {
        let inode = self.inodes.get_mut(&id).ok_or(FilesystemError::NotFound)?;
        if update == Metadata::default() {
            return Ok(());
        }
        inode.version = inode.version.wrapping_add(1);
        inode.ctime = self.now;
        inode.ctime_nanoseconds = 0;
        self.apply_metadata(id, update)?;
        self.emit_inode(ChangeKind::Metadata, id, self.mutation_source);
        Ok(())
    }

    fn parent(&self, path: &str) -> Result<(InodeId, String), FilesystemError> {
        let mut parts = Self::parts(path)?;
        let name = parts.pop().ok_or(FilesystemError::InvalidPath)?;
        let parent = self.lookup(&parts.join("/"))?;
        Ok((parent, name.to_owned()))
    }

    fn apply_metadata(&mut self, id: InodeId, metadata: Metadata) -> Result<(), FilesystemError> {
        let inode = self.inodes.get_mut(&id).ok_or(FilesystemError::NotFound)?;
        if let Some(mode) = metadata.mode {
            inode.mode = mode & 0o7777;
        }
        if let Some(uid) = metadata.uid {
            inode.uid = uid;
        }
        if let Some(gid) = metadata.gid {
            inode.gid = gid;
        }
        if let Some(atime) = metadata.atime {
            inode.atime = atime;
            inode.atime_nanoseconds = 0;
        }
        if let Some(mtime) = metadata.mtime {
            inode.mtime = mtime;
            inode.mtime_nanoseconds = 0;
        }
        if let Some(ctime) = metadata.ctime {
            inode.ctime = ctime;
            inode.ctime_nanoseconds = 0;
        }
        Ok(())
    }

    fn insert(
        &mut self,
        parent: InodeId,
        name: String,
        kind: InodeKind,
        mode: u32,
    ) -> Result<InodeId, FilesystemError> {
        let at_capacity = self.inodes.len() >= self.limits.max_inodes
            || self.directory_entries >= self.limits.max_directory_entries;
        let directory = self
            .inodes
            .get_mut(&parent)
            .ok_or(FilesystemError::NotFound)?;
        let InodeKind::Directory {
            entries,
            next_cookie,
        } = &mut directory.kind
        else {
            return Err(FilesystemError::NotDirectory);
        };
        if entries.contains_key(&name) {
            return Err(FilesystemError::AlreadyExists);
        }
        if at_capacity {
            return Err(FilesystemError::NoSpace);
        }
        let id = InodeId(self.next_inode);
        let next_inode = self
            .next_inode
            .checked_add(1)
            .ok_or(FilesystemError::NoSpace)?;
        let following_cookie = next_cookie.checked_add(1).ok_or(FilesystemError::NoSpace)?;
        entries.insert(
            name.clone(),
            DirectoryEntry {
                inode: id,
                cookie: *next_cookie,
            },
        );
        *next_cookie = following_cookie;
        self.next_inode = next_inode;
        Self::touch(directory, self.now);
        self.directory_entries += 1;
        let inode_parent = matches!(kind, InodeKind::Directory { .. }).then_some(parent);
        self.inodes.insert(
            id,
            Inode {
                id,
                kind,
                parent: inode_parent,
                mode,
                uid: 1000,
                gid: 1000,
                version: 0,
                link_count: 1,
                fid_refs: 0,
                atime: self.now,
                mtime: self.now,
                ctime: self.now,
                atime_nanoseconds: 0,
                mtime_nanoseconds: 0,
                ctime_nanoseconds: 0,
            },
        );
        self.links.entry(id).or_default().insert((parent, name));
        Ok(id)
    }

    /// Adds an empty directory to an existing parent.
    ///
    /// # Errors
    /// Returns a path, duplicate-entry, or quota error.
    pub fn mkdir(&mut self, path: &str) -> Result<InodeId, FilesystemError> {
        let (parent, name) = self.parent(path)?;
        self.mkdir_at(parent, &name, 0o755, 1000, 1000)
    }

    /// Adds a symbolic link without resolving its target.
    ///
    /// # Errors
    /// Returns a path, duplicate-entry, or quota error.
    pub fn symlink(&mut self, path: &str, target: &str) -> Result<InodeId, FilesystemError> {
        let (parent, name) = self.parent(path)?;
        self.symlink_at(parent, &name, target, 1000, 1000)
    }

    /// Returns a symbolic link's literal target.
    ///
    /// # Errors
    /// Returns a path or type error.
    pub fn readlink(&self, path: &str) -> Result<&str, FilesystemError> {
        let id = self.lookup(path)?;
        let inode = self.inodes.get(&id).ok_or(FilesystemError::NotFound)?;
        match &inode.kind {
            InodeKind::Symlink(target) => Ok(target),
            _ => Err(FilesystemError::InvalidPath),
        }
    }

    /// Replaces a whole file or creates one in an existing directory.
    ///
    /// # Errors
    /// Returns a path, type, or quota error without changing file bytes.
    pub fn write_file(&mut self, path: &str, bytes: &[u8]) -> Result<InodeId, FilesystemError> {
        if bytes.len() > self.limits.max_file_bytes {
            return Err(FilesystemError::FileTooLarge);
        }
        let (parent, name) = self.parent(path)?;
        let existing = match &self
            .inodes
            .get(&parent)
            .ok_or(FilesystemError::NotFound)?
            .kind
        {
            InodeKind::Directory { entries, .. } => entries.get(&name).map(|entry| entry.inode),
            _ => return Err(FilesystemError::NotDirectory),
        };
        let old_size = if let Some(id) = existing {
            match &self.inodes.get(&id).ok_or(FilesystemError::NotFound)?.kind {
                InodeKind::File(body) => body.len(),
                _ => return Err(FilesystemError::IsDirectory),
            }
        } else {
            0
        };
        let new_total = self.logical_bytes - old_size;
        let new_total = new_total
            .checked_add(bytes.len())
            .ok_or(FilesystemError::NoSpace)?;
        if new_total > self.limits.max_tree_bytes {
            return Err(FilesystemError::NoSpace);
        }
        let (id, kind) = if let Some(id) = existing {
            let inode = self.inodes.get_mut(&id).ok_or(FilesystemError::NotFound)?;
            inode.kind = InodeKind::File(bytes.to_vec());
            Self::touch(inode, self.now);
            (id, ChangeKind::Write)
        } else {
            (
                self.insert(parent, name, InodeKind::File(bytes.to_vec()), 0o644)?,
                ChangeKind::Create,
            )
        };
        self.logical_bytes = new_total;
        self.emit(kind, Some(id), path, None, self.mutation_source);
        Ok(id)
    }

    /// Removes a file or an empty directory without reusing its inode ID.
    ///
    /// # Errors
    /// Returns a path, type, or nonempty-directory error.
    pub fn remove(&mut self, path: &str) -> Result<(), FilesystemError> {
        let (parent, name) = self.parent(path)?;
        self.remove_at(parent, &name)
    }

    /// Removes one literal entry without invalidating retained inode fids.
    /// # Errors
    /// Reports invalid names, missing entries, or nonempty directories.
    pub fn remove_at(&mut self, parent: InodeId, name: &str) -> Result<(), FilesystemError> {
        let path = self.entry_path(parent, name)?;
        let name = name.to_owned();
        let id = match &self
            .inodes
            .get(&parent)
            .ok_or(FilesystemError::NotFound)?
            .kind
        {
            InodeKind::Directory { entries, .. } => {
                entries.get(&name).ok_or(FilesystemError::NotFound)?.inode
            }
            _ => return Err(FilesystemError::NotDirectory),
        };
        let inode = self.inodes.get(&id).ok_or(FilesystemError::NotFound)?;
        if let InodeKind::Directory { entries, .. } = &inode.kind
            && !entries.is_empty()
        {
            return Err(FilesystemError::NotEmpty);
        }
        let old_size = match &inode.kind {
            InodeKind::File(body) => body.len(),
            _ => 0,
        };
        let directory = self
            .inodes
            .get_mut(&parent)
            .ok_or(FilesystemError::NotFound)?;
        let InodeKind::Directory { entries, .. } = &mut directory.kind else {
            return Err(FilesystemError::NotDirectory);
        };
        entries.remove(&name);
        if let Some(links) = self.links.get_mut(&id) {
            links.remove(&(parent, name));
            if links.is_empty() {
                self.links.remove(&id);
            }
        }
        Self::touch(directory, self.now);
        self.directory_entries -= 1;
        let inode = self.inodes.get_mut(&id).ok_or(FilesystemError::NotFound)?;
        inode.link_count -= 1;
        inode.ctime = self.now;
        inode.ctime_nanoseconds = 0;
        inode.version = inode.version.wrapping_add(1);
        if inode.link_count == 0 {
            inode.parent = None;
        }
        if inode.link_count == 0 && inode.fid_refs == 0 {
            self.inodes.remove(&id);
            self.logical_bytes -= old_size;
            self.locks.retain(|lock| lock.inode != id);
        }
        self.emit(
            ChangeKind::Remove,
            Some(id),
            &path,
            None,
            self.mutation_source,
        );
        Ok(())
    }

    /// Creates another directory entry for a regular file or symlink.
    ///
    /// # Errors
    /// Returns a path, type, duplicate-entry, or quota error.
    pub fn hard_link(&mut self, existing: &str, new_path: &str) -> Result<(), FilesystemError> {
        let id = self.lookup(existing)?;
        let (parent, name) = self.parent(new_path)?;
        self.link_at(parent, &name, id)
    }

    /// Links an inode, including a retained inode without a current path.
    /// # Errors
    /// Reports directory targets, invalid names, duplicate entries, or quotas.
    pub fn link_at(
        &mut self,
        parent: InodeId,
        name: &str,
        id: InodeId,
    ) -> Result<(), FilesystemError> {
        let new_path = self.entry_path(parent, name)?;
        let name = name.to_owned();
        let inode = self.inodes.get(&id).ok_or(FilesystemError::NotFound)?;
        if matches!(inode.kind, InodeKind::Directory { .. }) {
            return Err(FilesystemError::IsDirectory);
        }
        let next_links = inode
            .link_count
            .checked_add(1)
            .ok_or(FilesystemError::NoSpace)?;
        let at_capacity = self.directory_entries >= self.limits.max_directory_entries;
        let directory = self
            .inodes
            .get_mut(&parent)
            .ok_or(FilesystemError::NotFound)?;
        let InodeKind::Directory {
            entries,
            next_cookie,
        } = &mut directory.kind
        else {
            return Err(FilesystemError::NotDirectory);
        };
        if entries.contains_key(&name) {
            return Err(FilesystemError::AlreadyExists);
        }
        if at_capacity {
            return Err(FilesystemError::NoSpace);
        }
        let following_cookie = next_cookie.checked_add(1).ok_or(FilesystemError::NoSpace)?;
        entries.insert(
            name.clone(),
            DirectoryEntry {
                inode: id,
                cookie: *next_cookie,
            },
        );
        *next_cookie = following_cookie;
        Self::touch(directory, self.now);
        self.directory_entries += 1;
        let inode = self.inodes.get_mut(&id).ok_or(FilesystemError::NotFound)?;
        inode.link_count = next_links;
        inode.ctime = self.now;
        inode.ctime_nanoseconds = 0;
        inode.version = inode.version.wrapping_add(1);
        self.links.entry(id).or_default().insert((parent, name));
        self.emit(
            ChangeKind::Create,
            Some(id),
            &new_path,
            None,
            self.mutation_source,
        );
        Ok(())
    }

    /// Moves an entry, replacing a compatible destination when present.
    ///
    /// # Errors
    /// Returns a path, type, nonempty-directory, or cookie exhaustion error.
    pub fn rename(&mut self, old_path: &str, new_path: &str) -> Result<(), FilesystemError> {
        let (old_parent, old_name) = self.parent(old_path)?;
        let (new_parent, new_name) = self.parent(new_path)?;
        self.rename_at(old_parent, &old_name, new_parent, &new_name)
    }

    /// Moves literal entries between directory inodes.
    /// # Errors
    /// Rejects cycles, incompatible replacements, invalid names, and quotas.
    pub fn rename_at(
        &mut self,
        old_parent: InodeId,
        old_name: &str,
        new_parent: InodeId,
        new_name: &str,
    ) -> Result<(), FilesystemError> {
        let old_path = self.entry_path(old_parent, old_name)?;
        let new_path = self.entry_path(new_parent, new_name)?;
        let old_name = old_name.to_owned();
        let new_name = new_name.to_owned();
        let old_entry = match &self
            .inodes
            .get(&old_parent)
            .ok_or(FilesystemError::NotFound)?
            .kind
        {
            InodeKind::Directory { entries, .. } => entries
                .get(&old_name)
                .ok_or(FilesystemError::NotFound)?
                .clone(),
            _ => return Err(FilesystemError::NotDirectory),
        };
        let moving = self
            .inodes
            .get(&old_entry.inode)
            .ok_or(FilesystemError::NotFound)?;
        if matches!(moving.kind, InodeKind::Directory { .. }) {
            let mut ancestor = Some(new_parent);
            while let Some(id) = ancestor {
                if id == moving.id {
                    return Err(FilesystemError::InvalidPath);
                }
                ancestor = self.parent_inode(id);
            }
        }
        let new_directory = self
            .inodes
            .get(&new_parent)
            .ok_or(FilesystemError::NotFound)?;
        let InodeKind::Directory {
            entries,
            next_cookie,
        } = &new_directory.kind
        else {
            return Err(FilesystemError::NotDirectory);
        };
        let replaced = entries.get(&new_name).map(|entry| entry.inode);
        if replaced == Some(old_entry.inode) {
            return Ok(());
        }
        if let Some(replaced_id) = replaced {
            let destination = self
                .inodes
                .get(&replaced_id)
                .ok_or(FilesystemError::NotFound)?;
            match (&moving.kind, &destination.kind) {
                (InodeKind::Directory { .. }, InodeKind::Directory { entries, .. })
                    if !entries.is_empty() =>
                {
                    return Err(FilesystemError::NotEmpty);
                }
                (InodeKind::Directory { .. }, InodeKind::Directory { .. })
                | (
                    InodeKind::File(_) | InodeKind::Symlink(_),
                    InodeKind::File(_) | InodeKind::Symlink(_),
                ) => {}
                (InodeKind::Directory { .. }, _) => return Err(FilesystemError::NotDirectory),
                (_, InodeKind::Directory { .. }) => return Err(FilesystemError::IsDirectory),
            }
        }
        let cookie = if old_parent == new_parent {
            old_entry.cookie
        } else {
            *next_cookie
        };
        if old_parent != new_parent {
            next_cookie.checked_add(1).ok_or(FilesystemError::NoSpace)?;
        }

        // All fallible checks precede the entry changes, so readers never see
        // a half-moved path after an expected filesystem error.
        if replaced.is_some() {
            self.remove_at(new_parent, &new_name)?;
        }
        let directory = self
            .inodes
            .get_mut(&old_parent)
            .ok_or(FilesystemError::NotFound)?;
        let InodeKind::Directory { entries, .. } = &mut directory.kind else {
            return Err(FilesystemError::NotDirectory);
        };
        entries.remove(&old_name);
        if let Some(links) = self.links.get_mut(&old_entry.inode) {
            links.remove(&(old_parent, old_name));
            links.insert((new_parent, new_name.clone()));
        }
        Self::touch(directory, self.now);
        let directory = self
            .inodes
            .get_mut(&new_parent)
            .ok_or(FilesystemError::NotFound)?;
        let InodeKind::Directory {
            entries,
            next_cookie,
        } = &mut directory.kind
        else {
            return Err(FilesystemError::NotDirectory);
        };
        entries.insert(
            new_name,
            DirectoryEntry {
                inode: old_entry.inode,
                cookie,
            },
        );
        if old_parent != new_parent {
            *next_cookie += 1;
        }
        Self::touch(directory, self.now);
        let inode = self
            .inodes
            .get_mut(&old_entry.inode)
            .ok_or(FilesystemError::NotFound)?;
        if matches!(inode.kind, InodeKind::Directory { .. }) {
            inode.parent = Some(new_parent);
        }
        inode.ctime = self.now;
        inode.ctime_nanoseconds = 0;
        inode.version = inode.version.wrapping_add(1);
        self.emit(
            ChangeKind::Rename,
            Some(old_entry.inode),
            &new_path,
            Some(&old_path),
            self.mutation_source,
        );
        Ok(())
    }

    /// Reads a copy of the resident file bytes.
    ///
    /// # Errors
    /// Returns a path or type error.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, FilesystemError> {
        let id = self.lookup(path)?;
        let inode = self.inodes.get(&id).ok_or(FilesystemError::NotFound)?;
        match &inode.kind {
            InodeKind::File(bytes) => Ok(bytes.clone()),
            _ => Err(FilesystemError::IsDirectory),
        }
    }

    /// Clears the namespace and gives every new inode a fresh identity.
    ///
    /// # Errors
    /// Returns an exhaustion error if the generation cannot advance.
    pub fn reset(&mut self) -> Result<(), FilesystemError> {
        let limits = self.limits;
        let next_inode = self.next_inode;
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(FilesystemError::NoSpace)?;
        let source = self.mutation_source;
        let record_changes = self.record_changes;
        let following_inode = next_inode.checked_add(1).ok_or(FilesystemError::NoSpace)?;
        let identity = self.identity.clone();
        let next_session = self.next_session;
        *self = Self::new(limits, self.now);
        self.identity = identity;
        self.next_session = next_session;
        self.replace_root(next_inode)?;
        self.next_inode = following_inode;
        self.generation = generation;
        self.mutation_source = source;
        self.record_changes = record_changes;
        self.emit(ChangeKind::Reset, None, "", None, self.mutation_source);
        Ok(())
    }

    // Every namespace replacement gets a fresh root QID as well as fresh leaves.
    // Validation precedes the swap so identity exhaustion is atomic.
    fn replace_root(&mut self, id: u64) -> Result<(), FilesystemError> {
        let next = id.checked_add(1).ok_or(FilesystemError::NoSpace)?;
        let mut root = self
            .inodes
            .remove(&self.root)
            .expect("namespace has a root");
        root.id = InodeId(id);
        self.root = root.id;
        self.inodes.insert(root.id, root);
        self.next_inode = next;
        Ok(())
    }
    // Literal entry names are shared by the host path and protocol interfaces.
    // Paths are reconstructed only for notifications, never for file I/O.
    pub(crate) fn entry_path(
        &self,
        parent: InodeId,
        name: &str,
    ) -> Result<String, FilesystemError> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
            return Err(FilesystemError::InvalidPath);
        }
        if name.len() > MAX_NAME_BYTES {
            return Err(FilesystemError::NameTooLong);
        }
        let directory = self.inode(parent).ok_or(FilesystemError::NotFound)?;
        if !matches!(directory.kind, InodeKind::Directory { .. }) {
            return Err(FilesystemError::NotDirectory);
        }
        if directory.link_count == 0 {
            return Err(FilesystemError::NotFound);
        }
        if !self.record_changes {
            return Ok(String::new());
        }
        let prefix = self
            .paths_of(parent)
            .into_iter()
            .next()
            .ok_or(FilesystemError::NotFound)?;
        Ok(if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        })
    }

    fn create_at(
        &mut self,
        parent: InodeId,
        name: &str,
        kind: InodeKind,
        mode: u32,
        uid: u32,
        gid: u32,
    ) -> Result<InodeId, FilesystemError> {
        let path = self.entry_path(parent, name)?;
        let directory = self.inode(parent).ok_or(FilesystemError::NotFound)?;
        let inherited_group = directory.mode & 0o2000 != 0;
        let gid = if inherited_group { directory.gid } else { gid };
        let mode = if inherited_group && matches!(kind, InodeKind::Directory { .. }) {
            mode | 0o2000
        } else {
            mode
        };
        let id = self.insert(parent, name.to_owned(), kind, mode & 0o7777)?;
        let inode = self.inodes.get_mut(&id).expect("created inode exists");
        inode.uid = uid;
        inode.gid = gid;
        self.emit(
            ChangeKind::Create,
            Some(id),
            &path,
            None,
            self.mutation_source,
        );
        Ok(id)
    }

    /// # Errors
    /// Reports invalid names, duplicate entries, missing parents, and quotas.
    pub fn create_file_at(
        &mut self,
        parent: InodeId,
        name: &str,
        mode: u32,
        uid: u32,
        gid: u32,
    ) -> Result<InodeId, FilesystemError> {
        self.create_at(parent, name, InodeKind::File(Vec::new()), mode, uid, gid)
    }

    /// # Errors
    /// Reports invalid names, duplicate entries, missing parents, and quotas.
    pub fn mkdir_at(
        &mut self,
        parent: InodeId,
        name: &str,
        mode: u32,
        uid: u32,
        gid: u32,
    ) -> Result<InodeId, FilesystemError> {
        self.create_at(
            parent,
            name,
            InodeKind::Directory {
                entries: BTreeMap::new(),
                next_cookie: 1,
            },
            mode,
            uid,
            gid,
        )
    }

    /// # Errors
    /// Reports invalid targets/names, missing parents, duplicate entries, or quotas.
    pub fn symlink_at(
        &mut self,
        parent: InodeId,
        name: &str,
        target: &str,
        uid: u32,
        gid: u32,
    ) -> Result<InodeId, FilesystemError> {
        if target.contains('\0') || target.len() > usize::from(u16::MAX) {
            return Err(FilesystemError::InvalidPath);
        }
        self.create_at(
            parent,
            name,
            InodeKind::Symlink(target.to_owned()),
            0o777,
            uid,
            gid,
        )
    }

    /// # Errors
    /// Reports a missing inode or a non-directory target.
    pub fn directory_entries(&self, id: InodeId) -> Result<Vec<ListedEntry>, FilesystemError> {
        let InodeKind::Directory { entries, .. } =
            &self.inode(id).ok_or(FilesystemError::NotFound)?.kind
        else {
            return Err(FilesystemError::NotDirectory);
        };
        let mut result: Vec<_> = entries
            .iter()
            .map(|(name, entry)| ListedEntry {
                name: name.clone(),
                inode: entry.inode,
                cookie: entry.cookie,
            })
            .collect();
        result.sort_by_key(|entry| entry.cookie);
        Ok(result)
    }

    /// # Errors
    /// Reports a missing inode or a non-regular-file target.
    pub fn file_body(&self, id: InodeId) -> Result<&[u8], FilesystemError> {
        match &self.inode(id).ok_or(FilesystemError::NotFound)?.kind {
            InodeKind::File(body) => Ok(body),
            _ => Err(FilesystemError::IsDirectory),
        }
    }

    /// Reads only the requested range, borrowing resident bytes until reply copy.
    /// # Errors
    /// Reports missing inodes, wrong types, or invalid ranges.
    pub fn read_inode(
        &mut self,
        id: InodeId,
        offset: u64,
        count: usize,
        update_atime: bool,
    ) -> Result<&[u8], FilesystemError> {
        let inode = self.inodes.get_mut(&id).ok_or(FilesystemError::NotFound)?;
        let InodeKind::File(body) = &inode.kind else {
            return Err(FilesystemError::IsDirectory);
        };
        if count == 0
            || offset >= u64::try_from(body.len()).map_err(|_| FilesystemError::FileTooLarge)?
        {
            return Ok(&[]);
        }
        let bytes = body;
        if update_atime {
            inode.atime = self.now;
            inode.atime_nanoseconds = 0;
        }
        let start = usize::try_from(offset).map_err(|_| FilesystemError::FileTooLarge)?;
        let end = start + count.min(bytes.len() - start);
        Ok(&bytes[start..end])
    }

    // Size and quota checks happen before either loading or changing metadata.
    // Logical bytes include unlinked files retained by open fids.
    /// # Errors
    /// Reports wrong types, oversized files, or exhausted tree quota.
    pub fn validate_resize(&self, id: InodeId, size: u64) -> Result<usize, FilesystemError> {
        let old = self.file_body(id)?.len();
        let size = usize::try_from(size).map_err(|_| FilesystemError::FileTooLarge)?;
        if size > self.limits.max_file_bytes {
            return Err(FilesystemError::FileTooLarge);
        }
        let total = (self.logical_bytes - old)
            .checked_add(size)
            .ok_or(FilesystemError::NoSpace)?;
        if total > self.limits.max_tree_bytes {
            return Err(FilesystemError::NoSpace);
        }
        Ok(size)
    }

    /// # Errors
    /// Reports wrong types, invalid offsets, or exhausted file/tree quota.
    pub fn validate_write(
        &self,
        id: InodeId,
        offset: u64,
        count: usize,
        append: bool,
    ) -> Result<usize, FilesystemError> {
        let old = self.file_body(id)?.len();
        if count == 0 {
            return Ok(old);
        }
        let offset = if append {
            u64::try_from(old).map_err(|_| FilesystemError::FileTooLarge)?
        } else {
            offset
        };
        let end = offset
            .checked_add(u64::try_from(count).map_err(|_| FilesystemError::FileTooLarge)?)
            .ok_or(FilesystemError::FileTooLarge)?;
        self.validate_resize(
            id,
            end.max(u64::try_from(old).map_err(|_| FilesystemError::FileTooLarge)?),
        )
    }

    /// Writes through a retained inode; append chooses EOF at execution time.
    /// # Errors
    /// Reports quotas, wrong types, or invalid ranges.
    pub fn write_inode(
        &mut self,
        id: InodeId,
        offset: u64,
        data: &[u8],
        append: bool,
    ) -> Result<(), FilesystemError> {
        let size = self.validate_write(id, offset, data.len(), append)?;
        if data.is_empty() {
            return Ok(());
        }
        let inode = self.inodes.get_mut(&id).ok_or(FilesystemError::NotFound)?;
        let InodeKind::File(body) = &mut inode.kind else {
            return Err(FilesystemError::IsDirectory);
        };
        let old = body.len();
        let bytes = body;
        let start = if append {
            old
        } else {
            usize::try_from(offset).map_err(|_| FilesystemError::FileTooLarge)?
        };
        bytes.resize(size, 0);
        bytes[start..start + data.len()].copy_from_slice(data);
        self.logical_bytes = self.logical_bytes - old + size;
        Self::touch(inode, self.now);
        self.emit_inode(ChangeKind::Write, id, self.mutation_source);
        Ok(())
    }

    /// Applies a validated attribute transaction in the resident namespace.
    /// # Errors
    /// Checks size, type, and quotas before changing any selected field.
    pub fn update_attributes(
        &mut self,
        id: InodeId,
        update: AttributeUpdate,
    ) -> Result<(), FilesystemError> {
        let size = update
            .size
            .map(|size| self.validate_resize(id, size))
            .transpose()?;
        let inode = self.inodes.get_mut(&id).ok_or(FilesystemError::NotFound)?;
        if update == AttributeUpdate::default() {
            return Ok(());
        }
        if let Some(size) = size {
            let InodeKind::File(body) = &mut inode.kind else {
                return Err(FilesystemError::IsDirectory);
            };
            let old = body.len();
            let bytes = body;
            bytes.resize(size, 0);
            self.logical_bytes = self.logical_bytes - old + size;
            inode.mtime = self.now;
            inode.mtime_nanoseconds = 0;
        }

        // Explicit timestamps retain their precision; unselected values survive.
        // Every nonempty attribute update changes ctime and QID version once.
        if let Some(mode) = update.mode {
            inode.mode = mode & 0o7777;
        }
        if let Some(uid) = update.uid {
            inode.uid = uid;
        }
        if let Some(gid) = update.gid {
            inode.gid = gid;
        }
        let resolve = |time| match time {
            TimeUpdate::Current => FileTime {
                seconds: self.now,
                nanoseconds: 0,
            },
            TimeUpdate::Explicit(time) => time,
        };
        if let Some(time) = update.atime {
            let time = resolve(time);
            inode.atime = time.seconds();
            inode.atime_nanoseconds = time.nanoseconds();
        }
        if let Some(time) = update.mtime {
            let time = resolve(time);
            inode.mtime = time.seconds();
            inode.mtime_nanoseconds = time.nanoseconds();
        }
        inode.ctime = self.now;
        inode.ctime_nanoseconds = 0;
        inode.version = inode.version.wrapping_add(1);
        self.emit_inode(
            if size.is_some() {
                ChangeKind::Write
            } else {
                ChangeKind::Metadata
            },
            id,
            self.mutation_source,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ByteRangeLock, ChangeKind, ChangeSource, Filesystem, FilesystemError, Limits, LockKind,
    };

    // 9P2000.L Tlock/Tgetlock use fcntl process/client ownership and range
    // conversion. Sessions track cleanup, not the identity of a lock owner.
    #[test]
    fn locks_distinguish_owners_split_unlocks_and_convert_ranges() {
        let mut fs = Filesystem::new(Limits::default(), 100);
        let inode = fs.write_file("file", b"x").unwrap();
        let lock = ByteRangeLock {
            inode,
            session: 1,
            kind: LockKind::Write,
            start: 0,
            length: 100,
            process: 10,
            client: "host".into(),
        };
        let mut other = ByteRangeLock {
            process: 20,
            ..lock.clone()
        };
        assert!(fs.try_lock(lock.clone()).unwrap());
        assert!(!fs.try_lock(other.clone()).unwrap());
        fs.unlock(inode, 1, 10, "host", 40, 20).unwrap();
        other.start = 40;
        other.length = 20;
        assert!(fs.conflicting_lock(&other).unwrap().is_none());
        for start in [0, 39, 60, 99] {
            other.start = start;
            other.length = 1;
            assert!(fs.conflicting_lock(&other).unwrap().is_some());
        }

        // Downgrading only the middle leaves both exclusive outer intervals.
        let read = ByteRangeLock {
            kind: LockKind::Read,
            start: 20,
            length: 60,
            ..lock.clone()
        };
        assert!(fs.try_lock(read).unwrap());
        other.kind = LockKind::Read;
        other.start = 20;
        other.length = 60;
        assert!(fs.conflicting_lock(&other).unwrap().is_none());
        other.kind = LockKind::Write;
        assert!(fs.conflicting_lock(&other).unwrap().is_some());
        other.kind = LockKind::Read;
        other.start = 80;
        other.length = 1;
        assert!(fs.conflicting_lock(&other).unwrap().is_some());

        // The same process/client remains one owner across endpoint sessions.
        assert!(fs.try_lock(ByteRangeLock { session: 2, ..lock }).unwrap());
        fs.release_session_locks(1);
        assert!(fs.conflicting_lock(&other).unwrap().is_some());
        fs.release_session_locks(2);
        assert!(fs.conflicting_lock(&other).unwrap().is_none());
    }

    #[test]
    fn zero_length_lock_keeps_tail_after_unlock_near_offset_limit() {
        let mut fs = Filesystem::new(Limits::default(), 100);
        let inode = fs.write_file("file", b"").unwrap();
        let lock = ByteRangeLock {
            inode,
            session: 1,
            kind: LockKind::Write,
            start: 10,
            length: 0,
            process: 10,
            client: "host".into(),
        };
        fs.try_lock(lock.clone()).unwrap();
        fs.unlock(inode, 1, 10, "host", 20, 10).unwrap();
        let other = ByteRangeLock {
            process: 20,
            start: u64::MAX,
            length: 1,
            ..lock
        };
        assert!(fs.conflicting_lock(&other).unwrap().is_some());
        fs.unlock(inode, 1, 10, "host", 30, 0).unwrap();
        assert!(fs.conflicting_lock(&other).unwrap().is_none());
    }

    #[test]
    fn notifications_follow_aliases_through_directory_moves_and_preserve_origin() {
        let mut fs = Filesystem::new(Limits::default(), 100);
        fs.mkdir("dir").unwrap();
        let id = fs.write_file("dir/file", b"old").unwrap();
        fs.hard_link("dir/file", "alias").unwrap();
        fs.rename("dir", "moved").unwrap();
        assert_eq!(fs.paths_of(id), vec!["alias", "moved/file"]);
        while fs.next_change().is_some() {}
        fs.set_mutation_source(ChangeSource::HostOrigin(42));
        fs.write_file("/moved/file", b"new").unwrap();
        let changed = fs.next_change().unwrap();
        assert_eq!(changed.path, "moved/file");
        assert_eq!(changed.paths, vec!["alias", "moved/file"]);
        assert_eq!(changed.source, ChangeSource::HostOrigin(42));
        fs.set_mutation_source(ChangeSource::Guest);
        fs.remove("alias").unwrap();
        assert_eq!(fs.next_change().unwrap().source, ChangeSource::Guest);
        assert_eq!(fs.paths_of(id), vec!["moved/file"]);
        fs.remove("moved/file").unwrap();
        assert!(fs.paths_of(id).is_empty());
    }

    #[test]
    fn literal_posix_names_and_invalid_paths_agree_between_host_and_child_lookup() {
        let mut fs = Filesystem::new(Limits::default(), 100);
        let name = "file\\name";
        let id = fs.write_file(name, b"x").unwrap();
        assert_eq!(fs.child(fs.root(), name), Ok(id));
        assert_eq!(fs.lookup(name), Ok(id));
        for path in ["a//b", "a/../b", "a/./b", "a\0b"] {
            assert_eq!(fs.write_file(path, b"x"), Err(FilesystemError::InvalidPath));
        }
        assert_eq!(
            fs.symlink("link", "target\0suffix"),
            Err(FilesystemError::InvalidPath)
        );
        assert_eq!(fs.lookup("link"), Err(FilesystemError::NotFound));
    }

    #[test]
    fn notification_overflow_invalidates_view_and_tracking_can_be_disabled() {
        let mut fs = Filesystem::new(Limits::default(), 100);
        for _ in 0..2000 {
            fs.write_file("file", b"x").unwrap();
        }
        assert_eq!(fs.next_change().unwrap().kind, ChangeKind::Rescan);
        assert!(fs.next_change().is_none());
        fs.write_file("file", b"y").unwrap();
        assert_eq!(fs.next_change().unwrap().kind, ChangeKind::Write);
        fs.set_change_tracking(false);
        fs.write_file("file", b"z").unwrap();
        fs.reset().unwrap();
        assert!(fs.next_change().is_none());
        fs.set_change_tracking(true);
        assert_eq!(fs.next_change().unwrap().kind, ChangeKind::Rescan);
    }

    #[test]
    fn quotas_fail_without_modifying_existing_file() {
        let mut fs = Filesystem::new(
            Limits {
                max_file_bytes: 4,
                max_tree_bytes: 4,
                ..Limits::default()
            },
            100,
        );
        fs.write_file("file", b"1234").unwrap();
        assert_eq!(
            fs.write_file("file", b"12345"),
            Err(FilesystemError::FileTooLarge)
        );
        assert_eq!(fs.read_file("file"), Ok(b"1234".to_vec()));
    }

    #[test]
    fn directory_cookies_survive_removal_and_empty_directory_rule() {
        let mut fs = Filesystem::new(Limits::default(), 100);
        fs.mkdir("dir").unwrap();
        fs.write_file("dir/a", b"a").unwrap();
        fs.write_file("dir/b", b"b").unwrap();
        assert_eq!(fs.remove("dir"), Err(FilesystemError::NotEmpty));
        let old_cookie = fs.list_directory("dir").unwrap()[1].cookie;
        fs.remove("dir/a").unwrap();
        fs.write_file("dir/c", b"c").unwrap();
        let entries = fs.list_directory("dir").unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "c"]
        );
        assert_eq!(entries[0].cookie, old_cookie);
        assert!(entries[1].cookie > old_cookie);
        fs.remove("dir/b").unwrap();
        fs.remove("dir/c").unwrap();
        fs.remove("dir").unwrap();
        assert_eq!(fs.lookup("dir"), Err(FilesystemError::NotFound));
    }

    #[test]
    fn hard_links_share_content_and_keep_inode_alive() {
        let mut fs = Filesystem::new(
            Limits {
                max_tree_bytes: 4,
                ..Limits::default()
            },
            100,
        );
        let id = fs.write_file("first", b"data").unwrap();
        fs.hard_link("first", "second").unwrap();
        assert_eq!(fs.lookup("second"), Ok(id));
        fs.write_file("second", b"edit").unwrap();
        assert_eq!(fs.read_file("first"), Ok(b"edit".to_vec()));
        fs.remove("first").unwrap();
        assert_eq!(fs.read_file("second"), Ok(b"edit".to_vec()));
        fs.remove("second").unwrap();
        fs.write_file("replacement", b"next").unwrap();
        assert_ne!(fs.lookup("replacement"), Ok(id));
    }

    #[test]
    fn rename_moves_inode_and_rejects_directory_cycles() {
        let mut fs = Filesystem::new(Limits::default(), 100);
        fs.mkdir("left").unwrap();
        fs.mkdir("right").unwrap();
        fs.mkdir("left/child").unwrap();
        let id = fs.write_file("left/file", b"data").unwrap();
        fs.rename("left/file", "right/file").unwrap();
        assert_eq!(fs.lookup("right/file"), Ok(id));
        assert_eq!(fs.lookup("left/file"), Err(FilesystemError::NotFound));
        assert_eq!(
            fs.rename("left", "left/child/nested"),
            Err(FilesystemError::InvalidPath)
        );
        assert!(fs.lookup("left/child").is_ok());
        let child = fs.lookup("left/child").unwrap();
        fs.rename("left/child", "right/child").unwrap();
        assert_eq!(
            fs.inode(child).unwrap().parent,
            Some(fs.lookup("right").unwrap())
        );
    }

    #[test]
    fn rename_replacement_keeps_destination_unmodified_on_type_error() {
        let mut fs = Filesystem::new(Limits::default(), 100);
        fs.mkdir("directory").unwrap();
        fs.write_file("file", b"one").unwrap();
        assert_eq!(
            fs.rename("file", "directory"),
            Err(FilesystemError::IsDirectory)
        );
        assert_eq!(fs.read_file("file"), Ok(b"one".to_vec()));
        fs.write_file("replacement", b"two").unwrap();
        fs.rename("file", "replacement").unwrap();
        assert_eq!(fs.read_file("replacement"), Ok(b"one".to_vec()));
    }

    #[test]
    fn symlink_target_is_literal_and_not_followed_by_host_lookup() {
        let mut fs = Filesystem::new(Limits::default(), 100);
        fs.symlink("link", "../outside").unwrap();
        assert_eq!(fs.readlink("link"), Ok("../outside"));
        assert_eq!(fs.read_file("link"), Err(FilesystemError::IsDirectory));
    }

    #[test]
    fn unlinked_open_file_retains_quota_until_final_fid_closes() {
        let mut fs = Filesystem::new(
            Limits {
                max_tree_bytes: 4,
                ..Limits::default()
            },
            100,
        );
        let id = fs.write_file("open", b"data").unwrap();
        fs.retain_fid(id).unwrap();
        fs.remove("open").unwrap();
        assert!(fs.inode(id).is_some());
        assert_eq!(fs.write_file("new", b"x"), Err(FilesystemError::NoSpace));
        fs.release_fid(id).unwrap();
        fs.write_file("new", b"x").unwrap();
        assert!(fs.inode(id).is_none());
    }

    #[test]
    fn locks_conflict_across_sessions_and_release_on_close() {
        let mut fs = Filesystem::new(Limits::default(), 100);
        let inode = fs.write_file("file", b"data").unwrap();
        let first = ByteRangeLock {
            inode,
            session: 1,
            kind: LockKind::Write,
            start: u64::MAX - 2,
            length: 10,
            process: 7,
            client: "a".into(),
        };
        let second = ByteRangeLock {
            session: 2,
            kind: LockKind::Read,
            start: u64::MAX - 1,
            length: 0,
            process: 8,
            client: "b".into(),
            ..first.clone()
        };
        assert_eq!(fs.try_lock(first), Ok(true));
        assert_eq!(fs.try_lock(second.clone()), Ok(false));
        fs.release_session_locks(1);
        assert_eq!(fs.try_lock(second), Ok(true));
    }

    #[test]
    fn notifications_name_all_hard_links_and_retired_path() {
        let mut fs = Filesystem::new(Limits::default(), 100);
        fs.write_file("a", b"old").unwrap();
        fs.hard_link("a", "b").unwrap();
        while fs.next_change().is_some() {}
        fs.write_file("b", b"new").unwrap();
        let changed = fs.next_change().unwrap();
        assert_eq!(changed.kind, ChangeKind::Write);
        assert_eq!(changed.paths, vec!["a", "b"]);
        assert_eq!(changed.source, ChangeSource::Host);
        fs.remove("a").unwrap();
        let removed = fs.next_change().unwrap();
        assert_eq!(removed.path, "a");
        assert_eq!(removed.paths, vec!["a", "b"]);
    }
}
