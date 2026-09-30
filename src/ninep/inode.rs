//! Inode operations preserve open-file lifetime independently of path names.

use std::collections::BTreeMap;

use super::{
    AttributeUpdate, ChangeKind, FileBody, FileTime, Filesystem, FilesystemError, InodeId,
    InodeKind, ListedEntry, MAX_NAME_BYTES, TimeUpdate,
};

impl Filesystem {
    // Literal entry names are shared by the host path and protocol interfaces.
    // Paths are reconstructed only for notifications, never for file I/O.
    pub(super) fn entry_path(
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
        self.create_at(
            parent,
            name,
            InodeKind::File(FileBody::Resident(Vec::new())),
            mode,
            uid,
            gid,
        )
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
    pub fn file_body(&self, id: InodeId) -> Result<&FileBody, FilesystemError> {
        match &self.inode(id).ok_or(FilesystemError::NotFound)?.kind {
            InodeKind::File(body) => Ok(body),
            _ => Err(FilesystemError::IsDirectory),
        }
    }

    /// Reads only the requested range, borrowing resident bytes until reply copy.
    /// # Errors
    /// Reports missing inodes, wrong types, or bodies still needing a load.
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
        let bytes = match body {
            FileBody::Resident(bytes) => bytes,
            FileBody::Failed { .. } => return Err(FilesystemError::LoadFailed),
            _ => return Err(FilesystemError::NeedsLoad),
        };
        if update_atime {
            inode.atime = self.now;
            inode.atime_nanoseconds = 0;
        }
        let start = usize::try_from(offset).map_err(|_| FilesystemError::FileTooLarge)?;
        let end = start + count.min(bytes.len() - start);
        Ok(&bytes[start..end])
    }

    // Size and quota checks happen before either loading or changing metadata.
    // Logical bytes include unlinked files retained by fids or pending I/O.
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
    /// Reports quotas, wrong types, or bodies still needing a load.
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
        let bytes = match body {
            FileBody::Resident(bytes) => bytes,
            FileBody::Failed { .. } => return Err(FilesystemError::LoadFailed),
            _ => return Err(FilesystemError::NeedsLoad),
        };
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

    /// Applies a validated attribute transaction after any required lazy load.
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
            match body {
                FileBody::Resident(bytes) => bytes.resize(size, 0),
                _ if size == 0 => *body = FileBody::Resident(Vec::new()),
                _ if size == old => {}
                FileBody::Failed { .. } => return Err(FilesystemError::LoadFailed),
                _ => return Err(FilesystemError::NeedsLoad),
            }
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
