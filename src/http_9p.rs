//! Mutable 9P2000.L filesystem whose initial file bodies are fetched over HTTP.

use std::collections::{BTreeMap, VecDeque};

use crate::crypto::derive_key;
use crate::virtio_devices::{
    DeviceError, NinePBackend, NinePHostRequest, NinePRequestId, NinePRequestStatus,
};

const MAX_MESSAGE: usize = 64 * 1024;
const NOTAG: u16 = u16::MAX;
const RLERROR: u8 = 7;
const EIO: u32 = 5;
const EBADF: u32 = 9;
const EEXIST: u32 = 17;
const ENOENT: u32 = 2;
const ENOTDIR: u32 = 20;
const EISDIR: u32 = 21;
const EINVAL: u32 = 22;
const EFBIG: u32 = 27;
const ENOTEMPTY: u32 = 39;
const EPROTO: u32 = 71;
const EOPNOTSUPP: u32 = 95;

#[derive(Clone)]
enum NodeKind {
    Directory(BTreeMap<String, usize>),
    File(FileData),
    Symlink(String),
    Command,
    Special,
}

#[derive(Clone)]
enum FileData {
    Remote { id: u64, size: u64 },
    Loading { size: u64 },
    Local(Vec<u8>),
}

#[derive(Clone)]
struct Node {
    parent: usize,
    name: String,
    mode: u32,
    uid: u32,
    gid: u32,
    mtime: u64,
    version: u32,
    kind: NodeKind,
}

#[derive(Clone, Copy)]
struct Fid {
    node: usize,
    flags: u32,
}

enum Pending {
    Head,
    FileList,
    File(usize),
}

/// Browser HTTP filesystem compatible with `TinyEMU`'s `fs_net` image layout.
pub struct HttpNineP {
    base_url: String,
    password: String,
    nodes: Vec<Node>,
    fids: BTreeMap<u32, Fid>,
    msize: usize,
    next_request: u32,
    outgoing: VecDeque<NinePHostRequest>,
    pending: BTreeMap<u32, Pending>,
    command_replies: BTreeMap<u32, Vec<u8>>,
    initialized: bool,
}

impl HttpNineP {
    #[must_use]
    pub fn new(base_url: &str, password: String) -> Self {
        let mut result = Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            password,
            nodes: Vec::new(),
            fids: BTreeMap::new(),
            msize: MAX_MESSAGE,
            next_request: 1,
            outgoing: VecDeque::new(),
            pending: BTreeMap::new(),
            command_replies: BTreeMap::new(),
            initialized: false,
        };
        result.reset_tree();
        result.queue("head", Pending::Head);
        result
    }

    fn reset_tree(&mut self) {
        self.nodes.clear();
        self.nodes.push(Node {
            parent: 0,
            name: String::new(),
            mode: 0o755,
            uid: 0,
            gid: 0,
            mtime: 0,
            version: 0,
            kind: NodeKind::Directory(BTreeMap::new()),
        });
    }

    fn queue(&mut self, suffix: &str, pending: Pending) {
        let id = self.next_request;
        self.next_request = self.next_request.wrapping_add(1).max(1);
        self.outgoing.push_back(NinePHostRequest {
            id,
            url: format!("{}/{}", self.base_url, suffix),
        });
        self.pending.insert(id, pending);
    }

    fn request_file(&mut self, node: usize) {
        let NodeKind::File(FileData::Remote { id, size }) = self.nodes[node].kind else {
            return;
        };
        self.nodes[node].kind = NodeKind::File(FileData::Loading { size });
        self.queue(&format!("files/{id:016x}"), Pending::File(node));
    }

    fn add_node(&mut self, parent: usize, mut node: Node) -> Result<usize, P9Error> {
        let index = self.nodes.len();
        let NodeKind::Directory(children) = &mut self.nodes[parent].kind else {
            return Err(P9Error(ENOTDIR));
        };
        if children.contains_key(&node.name) {
            return Err(P9Error(EEXIST));
        }
        node.parent = parent;
        children.insert(node.name.clone(), index);
        self.nodes.push(node);
        Ok(index)
    }

    fn parse_file_list(&mut self, source: &str) -> Result<(), P9Error> {
        let mut lines = source.lines().peekable();
        let header = lines.next().ok_or(P9Error(EPROTO))?;
        if !header.contains("Version: 1") && !header.contains("Version:1") {
            return Err(P9Error(EPROTO));
        }
        self.reset_tree();
        self.parse_directory(&mut lines, 0)?;
        self.add_node(
            0,
            Node {
                parent: 0,
                name: ".fscmd".into(),
                mode: 0o666,
                uid: 0,
                gid: 0,
                mtime: 0,
                version: 0,
                kind: NodeKind::Command,
            },
        )?;
        if !self.password.is_empty() {
            let password = self.password.as_bytes().to_vec();
            self.add_node(
                0,
                Node {
                    parent: 0,
                    name: ".fscmd_pwd".into(),
                    mode: 0o600,
                    uid: 0,
                    gid: 0,
                    mtime: 0,
                    version: 0,
                    kind: NodeKind::File(FileData::Local(password)),
                },
            )?;
        }
        self.initialized = true;
        Ok(())
    }

    fn parse_directory<'a, I>(
        &mut self,
        lines: &mut std::iter::Peekable<I>,
        parent: usize,
    ) -> Result<(), P9Error>
    where
        I: Iterator<Item = &'a str>,
    {
        while let Some(line) = lines.next() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == "." {
                return Ok(());
            }
            let fields = split_fields(line)?;
            if fields.len() < 5 {
                return Err(P9Error(EPROTO));
            }
            let raw_mode = u32::from_str_radix(&fields[0], 8).map_err(|_| P9Error(EPROTO))?;
            let file_type = raw_mode >> 12;
            let uid = fields[1].parse().map_err(|_| P9Error(EPROTO))?;
            let gid = fields[2].parse().map_err(|_| P9Error(EPROTO))?;
            let mut at = 3;
            let (kind, recurse) = match file_type {
                4 => (NodeKind::Directory(BTreeMap::new()), true),
                8 => {
                    let size: u64 = fields[at].parse().map_err(|_| P9Error(EPROTO))?;
                    at += 1;
                    let file_id = if size == 0 {
                        0
                    } else {
                        let value = fields.get(at + 2).ok_or(P9Error(EPROTO))?;
                        u64::from_str_radix(value, 16).map_err(|_| P9Error(EPROTO))?
                    };
                    let data = if size == 0 {
                        FileData::Local(Vec::new())
                    } else {
                        FileData::Remote { id: file_id, size }
                    };
                    (NodeKind::File(data), false)
                }
                10 => {
                    let target = fields.get(at + 2).ok_or(P9Error(EPROTO))?.clone();
                    (NodeKind::Symlink(target), false)
                }
                _ => (NodeKind::Special, false),
            };
            let time = fields.get(at).ok_or(P9Error(EPROTO))?;
            let mtime = time
                .split_once('.')
                .map_or(time.as_str(), |(seconds, _)| seconds)
                .parse()
                .map_err(|_| P9Error(EPROTO))?;
            let name = fields.get(at + 1).ok_or(P9Error(EPROTO))?.clone();
            validate_name(&name)?;
            let child = self.add_node(
                parent,
                Node {
                    parent,
                    name,
                    mode: raw_mode & 0xfff,
                    uid,
                    gid,
                    mtime,
                    version: 0,
                    kind,
                },
            )?;
            if recurse {
                self.parse_directory(lines, child)?;
            }
        }
        if parent == 0 {
            Ok(())
        } else {
            Err(P9Error(EPROTO))
        }
    }

    fn process(&mut self, request: &[u8]) -> Result<NinePRequestStatus, DeviceError> {
        let mut reader = Reader::new(request);
        let declared = reader.u32().map_err(|_| DeviceError::InvalidRequest)? as usize;
        let message_type = reader.u8().map_err(|_| DeviceError::InvalidRequest)?;
        let tag = reader.u16().map_err(|_| DeviceError::InvalidRequest)?;
        if declared != request.len() || declared < 7 || declared > self.msize {
            return Err(DeviceError::InvalidRequest);
        }
        if !self.initialized {
            return Ok(NinePRequestStatus::Pending);
        }
        let mut writer = Writer::new(self.msize, message_type.wrapping_add(1), tag);
        let result = self.handle(message_type, tag, &mut reader, &mut writer);
        match result {
            Ok(HandleStatus::Complete) => {
                reader.done().map_err(|_| DeviceError::InvalidRequest)?;
                Ok(NinePRequestStatus::Complete(writer.finish()))
            }
            Ok(HandleStatus::Pending) => Ok(NinePRequestStatus::Pending),
            Err(P9Error(errno)) => {
                let mut error = Writer::new(self.msize, RLERROR, tag);
                error.u32(errno).map_err(|_| DeviceError::Backend)?;
                Ok(NinePRequestStatus::Complete(error.finish()))
            }
        }
    }

    fn handle(
        &mut self,
        ty: u8,
        tag: u16,
        r: &mut Reader<'_>,
        w: &mut Writer,
    ) -> Result<HandleStatus, P9Error> {
        match ty {
            8 => self.statfs(r, w)?,
            12 => self.lopen(r, w)?,
            14 => self.lcreate(r, w)?,
            16 => self.symlink(r, w)?,
            22 => self.readlink(r, w)?,
            24 => self.getattr(r, w)?,
            26 => self.setattr(r)?,
            40 => self.readdir(r, w)?,
            50 => {
                self.fid(r.u32()?)?;
                r.u32()?;
            }
            52 => {
                self.fid(r.u32()?)?;
                r.u8()?;
                r.u32()?;
                r.u64()?;
                r.u64()?;
                r.u32()?;
                r.string()?;
                w.u8(0)?;
            }
            54 => {
                self.fid(r.u32()?)?;
                r.u8()?;
                r.u64()?;
                r.u64()?;
                r.u32()?;
                r.string()?;
                w.u8(2)?;
                w.u64(0)?;
                w.u64(0)?;
                w.u32(0)?;
                w.string("")?;
            }
            72 => self.mkdir(r, w)?,
            74 => self.renameat(r)?,
            76 => self.unlinkat(r)?,
            100 => self.version(tag, r, w)?,
            104 => self.attach(r, w)?,
            108 => {
                r.u16()?;
            }
            110 => self.walk(r, w)?,
            116 => return self.read(r, w),
            118 => return self.write(r, w),
            120 => {
                let fid = r.u32()?;
                self.fid(fid)?;
                self.fids.remove(&fid);
                self.command_replies.remove(&fid);
            }
            _ => return Err(P9Error(EOPNOTSUPP)),
        }
        Ok(HandleStatus::Complete)
    }

    fn fid(&self, number: u32) -> Result<Fid, P9Error> {
        self.fids.get(&number).copied().ok_or(P9Error(EBADF))
    }
    fn directory(&self, node: usize) -> Result<&BTreeMap<String, usize>, P9Error> {
        match &self.nodes[node].kind {
            NodeKind::Directory(c) => Ok(c),
            _ => Err(P9Error(ENOTDIR)),
        }
    }
    fn child(&self, node: usize, name: &str) -> Result<usize, P9Error> {
        self.directory(node)?
            .get(name)
            .copied()
            .ok_or(P9Error(ENOENT))
    }
    fn qid(&self, w: &mut Writer, node: usize) -> Result<(), P9Error> {
        let kind = match self.nodes[node].kind {
            NodeKind::Directory(_) => 0x80,
            NodeKind::Symlink(_) => 2,
            _ => 0,
        };
        w.u8(kind)?;
        w.u32(self.nodes[node].version)?;
        w.u64(node as u64)?;
        Ok(())
    }
    fn size(&self, node: usize) -> u64 {
        match &self.nodes[node].kind {
            NodeKind::File(FileData::Remote { size, .. } | FileData::Loading { size, .. }) => *size,
            NodeKind::File(FileData::Local(data)) => data.len() as u64,
            NodeKind::Symlink(target) => target.len() as u64,
            _ => 0,
        }
    }
    fn node_mode(&self, node: usize) -> u32 {
        let kind = match self.nodes[node].kind {
            NodeKind::Directory(_) => 0o040_000,
            NodeKind::Symlink(_) => 0o120_000,
            NodeKind::File(_) | NodeKind::Command => 0o100_000,
            NodeKind::Special => 0,
        };
        kind | self.nodes[node].mode
    }

    fn version(&mut self, tag: u16, r: &mut Reader<'_>, w: &mut Writer) -> Result<(), P9Error> {
        if tag != NOTAG {
            return Err(P9Error(EPROTO));
        }
        let requested = r.u32()? as usize;
        let version = r.string()?;
        if requested < 256 {
            return Err(P9Error(EINVAL));
        }
        self.msize = requested.min(MAX_MESSAGE);
        self.fids.clear();
        w.u32(as_u32(self.msize)?)?;
        w.string(if version == "9P2000.L" {
            &version
        } else {
            "unknown"
        })
    }
    fn attach(&mut self, r: &mut Reader<'_>, w: &mut Writer) -> Result<(), P9Error> {
        let fid = r.u32()?;
        r.u32()?;
        r.string()?;
        r.string()?;
        r.u32()?;
        if self.fids.insert(fid, Fid { node: 0, flags: 0 }).is_some() {
            return Err(P9Error(EEXIST));
        }
        self.qid(w, 0)
    }
    fn walk(&mut self, r: &mut Reader<'_>, w: &mut Writer) -> Result<(), P9Error> {
        let fid = r.u32()?;
        let new_fid = r.u32()?;
        let count = r.u16()?;
        let mut node = self.fid(fid)?.node;
        if new_fid != fid && self.fids.contains_key(&new_fid) {
            return Err(P9Error(EEXIST));
        }
        let mut qids = Vec::new();
        for index in 0..count {
            let name = r.string()?;
            let next = if name == "." {
                Ok(node)
            } else if name == ".." {
                Ok(self.nodes[node].parent)
            } else {
                self.child(node, &name)
            };
            match next {
                Ok(value) => {
                    node = value;
                    qids.push(node);
                }
                Err(error) if index != 0 => break,
                Err(error) => return Err(error),
            }
        }
        self.fids.insert(new_fid, Fid { node, flags: 0 });
        w.u16(u16::try_from(qids.len()).map_err(|_| P9Error(EFBIG))?)?;
        for node in qids {
            self.qid(w, node)?;
        }
        Ok(())
    }
    fn lopen(&mut self, r: &mut Reader<'_>, w: &mut Writer) -> Result<(), P9Error> {
        let number = r.u32()?;
        let flags = r.u32()?;
        let mut fid = self.fid(number)?;
        if flags & 0x200 != 0 {
            self.resize(fid.node, 0)?;
        }
        fid.flags = flags;
        self.fids.insert(number, fid);
        self.qid(w, fid.node)?;
        w.u32(as_u32(self.msize - 24)?)
    }
    fn read(&mut self, r: &mut Reader<'_>, w: &mut Writer) -> Result<HandleStatus, P9Error> {
        let fid_number = r.u32()?;
        let node = self.fid(fid_number)?.node;
        let offset = r.u64()?;
        let count = r.u32()? as usize;
        match &self.nodes[node].kind {
            NodeKind::File(FileData::Remote { .. }) => {
                self.request_file(node);
                Ok(HandleStatus::Pending)
            }
            NodeKind::File(FileData::Loading { .. }) => Ok(HandleStatus::Pending),
            NodeKind::File(FileData::Local(data)) => {
                let start = usize::try_from(offset)
                    .map_err(|_| P9Error(EFBIG))?
                    .min(data.len());
                let length = count
                    .min(data.len() - start)
                    .min(w.remaining().saturating_sub(4));
                w.u32(as_u32(length)?)?;
                w.bytes(&data[start..start + length])?;
                Ok(HandleStatus::Complete)
            }
            NodeKind::Command => {
                let data = self
                    .command_replies
                    .get(&fid_number)
                    .map_or(&[][..], Vec::as_slice);
                let start = usize::try_from(offset)
                    .map_err(|_| P9Error(EFBIG))?
                    .min(data.len());
                let length = count
                    .min(data.len() - start)
                    .min(w.remaining().saturating_sub(4));
                w.u32(as_u32(length)?)?;
                w.bytes(&data[start..start + length])?;
                Ok(HandleStatus::Complete)
            }
            _ => Err(P9Error(EISDIR)),
        }
    }
    fn write(&mut self, r: &mut Reader<'_>, w: &mut Writer) -> Result<HandleStatus, P9Error> {
        let fid_number = r.u32()?;
        let fid = self.fid(fid_number)?;
        let mut offset = usize::try_from(r.u64()?).map_err(|_| P9Error(EFBIG))?;
        let count = r.u32()? as usize;
        let bytes = r.bytes(count)?;
        if matches!(self.nodes[fid.node].kind, NodeKind::Command) {
            self.command(fid_number, bytes)?;
            w.u32(as_u32(count)?)?;
            return Ok(HandleStatus::Complete);
        }
        match self.nodes[fid.node].kind {
            NodeKind::File(FileData::Remote { .. }) => {
                self.request_file(fid.node);
                return Ok(HandleStatus::Pending);
            }
            NodeKind::File(FileData::Loading { .. }) => return Ok(HandleStatus::Pending),
            NodeKind::File(FileData::Local(_)) => {}
            _ => return Err(P9Error(EISDIR)),
        }
        let NodeKind::File(FileData::Local(data)) = &mut self.nodes[fid.node].kind else {
            unreachable!()
        };
        if fid.flags & 0x400 != 0 {
            offset = data.len();
        }
        let end = offset.checked_add(count).ok_or(P9Error(EFBIG))?;
        if end > 16 << 20 {
            return Err(P9Error(EFBIG));
        }
        data.resize(data.len().max(end), 0);
        data[offset..end].copy_from_slice(bytes);
        self.nodes[fid.node].version = self.nodes[fid.node].version.wrapping_add(1);
        w.u32(as_u32(count)?)?;
        Ok(HandleStatus::Complete)
    }

    fn command(&mut self, fid: u32, bytes: &[u8]) -> Result<(), P9Error> {
        let source = std::str::from_utf8(bytes).map_err(|_| P9Error(EINVAL))?;
        let fields = split_fields(source)?;
        if fields.first().map(String::as_str) != Some("pbkdf2") || fields.len() != 5 {
            return Err(P9Error(EIO));
        }
        let password = decode_hex(&fields[1])?;
        let salt = decode_hex(&fields[2])?;
        let iterations = fields[3].parse().map_err(|_| P9Error(EINVAL))?;
        let length: usize = fields[4].parse().map_err(|_| P9Error(EINVAL))?;
        if length == 0 || length > 4096 {
            return Err(P9Error(EINVAL));
        }
        let mut reply = vec![0; length];
        derive_key(&password, &salt, iterations, &mut reply).map_err(|_| P9Error(EINVAL))?;
        self.command_replies.insert(fid, reply);
        Ok(())
    }
    fn statfs(&self, r: &mut Reader<'_>, w: &mut Writer) -> Result<(), P9Error> {
        self.fid(r.u32()?)?;
        w.u32(0x0102_1997)?;
        w.u32(4096)?;
        for value in [0x10_0000, 0x0f_0000, 0x0f_0000, 0x10_0000, 0x0f_0000, 1] {
            w.u64(value)?;
        }
        w.u32(255)
    }
    fn getattr(&self, r: &mut Reader<'_>, w: &mut Writer) -> Result<(), P9Error> {
        let node = self.fid(r.u32()?)?.node;
        let requested = r.u64()?;
        let n = &self.nodes[node];
        w.u64(requested)?;
        self.qid(w, node)?;
        w.u32(self.node_mode(node))?;
        w.u32(n.uid)?;
        w.u32(n.gid)?;
        w.u64(1)?;
        w.u64(0)?;
        w.u64(self.size(node))?;
        w.u64(4096)?;
        w.u64(self.size(node).div_ceil(512))?;
        for value in [
            n.mtime,
            0,
            n.mtime,
            0,
            n.mtime,
            0,
            0,
            0,
            0,
            u64::from(n.version),
        ] {
            w.u64(value)?;
        }
        Ok(())
    }
    fn readdir(&self, r: &mut Reader<'_>, w: &mut Writer) -> Result<(), P9Error> {
        let node = self.fid(r.u32()?)?.node;
        let offset = usize::try_from(r.u64()?).map_err(|_| P9Error(EFBIG))?;
        let count = r.u32()? as usize;
        let children: Vec<_> = self.directory(node)?.values().copied().collect();
        let mut payload = Writer::payload(count);
        for (index, child) in children.into_iter().enumerate().skip(offset) {
            let needed = 24 + self.nodes[child].name.len();
            if payload.remaining() < needed {
                break;
            }
            self.qid(&mut payload, child)?;
            payload.u64((index + 1) as u64)?;
            payload.u8(match self.nodes[child].kind {
                NodeKind::Directory(_) => 4,
                NodeKind::Symlink(_) => 10,
                _ => 8,
            })?;
            payload.string(&self.nodes[child].name)?;
        }
        w.u32(as_u32(payload.data.len())?)?;
        w.bytes(&payload.data)
    }
    fn readlink(&self, r: &mut Reader<'_>, w: &mut Writer) -> Result<(), P9Error> {
        let node = self.fid(r.u32()?)?.node;
        if let NodeKind::Symlink(target) = &self.nodes[node].kind {
            w.string(target)
        } else {
            Err(P9Error(EINVAL))
        }
    }
    fn resize(&mut self, node: usize, size: usize) -> Result<(), P9Error> {
        if size > 16 << 20 {
            return Err(P9Error(EFBIG));
        }
        let NodeKind::File(data) = &mut self.nodes[node].kind else {
            return Err(P9Error(EISDIR));
        };
        let mut local = match data {
            FileData::Local(v) => std::mem::take(v),
            _ if size == 0 => Vec::new(),
            _ => return Err(P9Error(EIO)),
        };
        local.resize(size, 0);
        *data = FileData::Local(local);
        Ok(())
    }
    fn setattr(&mut self, r: &mut Reader<'_>) -> Result<(), P9Error> {
        let node = self.fid(r.u32()?)?.node;
        let mask = r.u32()?;
        let mode = r.u32()?;
        let uid = r.u32()?;
        let gid = r.u32()?;
        let size = usize::try_from(r.u64()?).map_err(|_| P9Error(EFBIG))?;
        let atime = r.u64()?;
        r.u64()?;
        let mtime = r.u64()?;
        r.u64()?;
        if mask & 1 != 0 {
            self.nodes[node].mode = mode & 0xfff;
        }
        if mask & 2 != 0 {
            self.nodes[node].uid = uid;
        }
        if mask & 4 != 0 {
            self.nodes[node].gid = gid;
        }
        if mask & 8 != 0 {
            self.resize(node, size)?;
        }
        if mask & 0x20 != 0 {
            self.nodes[node].mtime = mtime;
        } else {
            let _ = atime;
        }
        Ok(())
    }
    fn create_node(
        &mut self,
        parent: usize,
        name: String,
        mode: u32,
        kind: NodeKind,
    ) -> Result<usize, P9Error> {
        validate_name(&name)?;
        self.add_node(
            parent,
            Node {
                parent,
                name,
                mode: mode & 0xfff,
                uid: 0,
                gid: 0,
                mtime: 0,
                version: 0,
                kind,
            },
        )
    }
    fn lcreate(&mut self, r: &mut Reader<'_>, w: &mut Writer) -> Result<(), P9Error> {
        let number = r.u32()?;
        let mut fid = self.fid(number)?;
        self.directory(fid.node)?;
        let name = r.string()?;
        let flags = r.u32()?;
        let mode = r.u32()?;
        r.u32()?;
        let node = self.create_node(
            fid.node,
            name,
            mode,
            NodeKind::File(FileData::Local(Vec::new())),
        )?;
        fid = Fid { node, flags };
        self.fids.insert(number, fid);
        self.qid(w, node)?;
        w.u32(as_u32(self.msize - 24)?)
    }
    fn mkdir(&mut self, r: &mut Reader<'_>, w: &mut Writer) -> Result<(), P9Error> {
        let parent = self.fid(r.u32()?)?.node;
        let name = r.string()?;
        let mode = r.u32()?;
        r.u32()?;
        let node = self.create_node(parent, name, mode, NodeKind::Directory(BTreeMap::new()))?;
        self.qid(w, node)
    }
    fn symlink(&mut self, r: &mut Reader<'_>, w: &mut Writer) -> Result<(), P9Error> {
        let parent = self.fid(r.u32()?)?.node;
        let name = r.string()?;
        let target = r.string()?;
        r.u32()?;
        let node = self.create_node(parent, name, 0o777, NodeKind::Symlink(target))?;
        self.qid(w, node)
    }
    fn unlinkat(&mut self, r: &mut Reader<'_>) -> Result<(), P9Error> {
        let parent = self.fid(r.u32()?)?.node;
        let name = r.string()?;
        let flags = r.u32()?;
        let node = self.child(parent, &name)?;
        let is_dir = matches!(self.nodes[node].kind, NodeKind::Directory(_));
        if is_dir != (flags & 0x200 != 0) {
            return Err(P9Error(if is_dir { EISDIR } else { ENOTDIR }));
        }
        if matches!(&self.nodes[node].kind,NodeKind::Directory(c)if !c.is_empty()) {
            return Err(P9Error(ENOTEMPTY));
        }
        let NodeKind::Directory(c) = &mut self.nodes[parent].kind else {
            return Err(P9Error(ENOTDIR));
        };
        c.remove(&name);
        Ok(())
    }
    fn renameat(&mut self, r: &mut Reader<'_>) -> Result<(), P9Error> {
        let old_parent = self.fid(r.u32()?)?.node;
        let old_name = r.string()?;
        let new_parent = self.fid(r.u32()?)?.node;
        let new_name = r.string()?;
        validate_name(&new_name)?;
        let node = self.child(old_parent, &old_name)?;
        if self.child(new_parent, &new_name).is_ok() {
            return Err(P9Error(EEXIST));
        }
        let NodeKind::Directory(old) = &mut self.nodes[old_parent].kind else {
            return Err(P9Error(ENOTDIR));
        };
        old.remove(&old_name);
        let NodeKind::Directory(new) = &mut self.nodes[new_parent].kind else {
            return Err(P9Error(ENOTDIR));
        };
        new.insert(new_name.clone(), node);
        self.nodes[node].parent = new_parent;
        self.nodes[node].name = new_name;
        Ok(())
    }
}

impl NinePBackend for HttpNineP {
    fn transact(
        &mut self,
        _: NinePRequestId,
        request: &[u8],
        _: u32,
    ) -> Result<NinePRequestStatus, DeviceError> {
        self.process(request)
    }
    fn next_request(&mut self) -> Option<NinePHostRequest> {
        self.outgoing.pop_front()
    }
    fn complete_request(&mut self, id: u32, data: Vec<u8>) -> Result<(), DeviceError> {
        match self.pending.remove(&id).ok_or(DeviceError::Backend)? {
            Pending::Head => {
                let text = std::str::from_utf8(&data).map_err(|_| DeviceError::Backend)?;
                let root = text
                    .lines()
                    .find_map(|line| line.strip_prefix("RootID:"))
                    .map(str::trim)
                    .ok_or(DeviceError::Backend)?;
                let root = u64::from_str_radix(root, 16).map_err(|_| DeviceError::Backend)?;
                self.queue(&format!("files/{root:016x}"), Pending::FileList);
            }
            Pending::FileList => {
                let text = std::str::from_utf8(&data).map_err(|_| DeviceError::Backend)?;
                self.parse_file_list(text)
                    .map_err(|_| DeviceError::Backend)?;
            }
            Pending::File(node) => {
                let NodeKind::File(FileData::Loading { size: expected }) = self.nodes[node].kind
                else {
                    return Err(DeviceError::Backend);
                };
                if data.len() as u64 != expected {
                    return Err(DeviceError::Backend);
                }
                self.nodes[node].kind = NodeKind::File(FileData::Local(data));
            }
        }
        Ok(())
    }
}

enum HandleStatus {
    Complete,
    Pending,
}
#[derive(Clone, Copy, Debug)]
struct P9Error(u32);

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], P9Error> {
        let end = self.offset.checked_add(n).ok_or(P9Error(EPROTO))?;
        let value = self.data.get(self.offset..end).ok_or(P9Error(EPROTO))?;
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, P9Error> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, P9Error> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, P9Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, P9Error> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, P9Error> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| P9Error(EPROTO))
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], P9Error> {
        self.take(n)
    }
    fn done(&self) -> Result<(), P9Error> {
        if self.offset == self.data.len() {
            Ok(())
        } else {
            Err(P9Error(EPROTO))
        }
    }
}
struct Writer {
    data: Vec<u8>,
    limit: usize,
}
impl Writer {
    fn new(limit: usize, ty: u8, tag: u16) -> Self {
        let mut s = Self {
            data: vec![0; 4],
            limit,
        };
        s.u8(ty).unwrap();
        s.u16(tag).unwrap();
        s
    }
    fn payload(limit: usize) -> Self {
        Self {
            data: Vec::new(),
            limit,
        }
    }
    fn remaining(&self) -> usize {
        self.limit.saturating_sub(self.data.len())
    }
    fn bytes(&mut self, v: &[u8]) -> Result<(), P9Error> {
        if v.len() > self.remaining() {
            return Err(P9Error(EFBIG));
        }
        self.data.extend_from_slice(v);
        Ok(())
    }
    fn u8(&mut self, v: u8) -> Result<(), P9Error> {
        self.bytes(&[v])
    }
    fn u16(&mut self, v: u16) -> Result<(), P9Error> {
        self.bytes(&v.to_le_bytes())
    }
    fn u32(&mut self, v: u32) -> Result<(), P9Error> {
        self.bytes(&v.to_le_bytes())
    }
    fn u64(&mut self, v: u64) -> Result<(), P9Error> {
        self.bytes(&v.to_le_bytes())
    }
    fn string(&mut self, v: &str) -> Result<(), P9Error> {
        self.u16(u16::try_from(v.len()).map_err(|_| P9Error(EFBIG))?)?;
        self.bytes(v.as_bytes())
    }
    fn finish(mut self) -> Vec<u8> {
        let len = u32::try_from(self.data.len()).expect("9p message length fits in u32");
        self.data[..4].copy_from_slice(&len.to_le_bytes());
        self.data
    }
}

fn validate_name(name: &str) -> Result<(), P9Error> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        Err(P9Error(EINVAL))
    } else {
        Ok(())
    }
}

fn as_u32(value: usize) -> Result<u32, P9Error> {
    u32::try_from(value).map_err(|_| P9Error(EFBIG))
}
fn split_fields(line: &str) -> Result<Vec<String>, P9Error> {
    let mut result = Vec::new();
    let bytes = line.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if at == bytes.len() {
            break;
        }
        let mut field = Vec::new();
        if bytes[at] == b'"' {
            at += 1;
            loop {
                let byte = *bytes.get(at).ok_or(P9Error(EPROTO))?;
                at += 1;
                match byte {
                    b'"' => break,
                    b'\\' => {
                        let escaped = *bytes.get(at).ok_or(P9Error(EPROTO))?;
                        at += 1;
                        field.push(match escaped {
                            b'"' | b'\\' | b'\'' => escaped,
                            b'n' => b'\n',
                            b'r' => b'\r',
                            b't' => b'\t',
                            b'x' => {
                                let high = hex(*bytes.get(at).ok_or(P9Error(EPROTO))?)?;
                                let low = hex(*bytes.get(at + 1).ok_or(P9Error(EPROTO))?)?;
                                at += 2;
                                high << 4 | low
                            }
                            _ => return Err(P9Error(EPROTO)),
                        });
                    }
                    b'\n' => return Err(P9Error(EPROTO)),
                    value => field.push(value),
                }
            }
        } else {
            while bytes
                .get(at)
                .is_some_and(|byte| !byte.is_ascii_whitespace())
            {
                field.push(bytes[at]);
                at += 1;
            }
        }
        result.push(String::from_utf8(field).map_err(|_| P9Error(EPROTO))?);
    }
    Ok(result)
}

fn hex(value: u8) -> Result<u8, P9Error> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(P9Error(EPROTO)),
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>, P9Error> {
    if !value.len().is_multiple_of(2) {
        return Err(P9Error(EINVAL));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok(hex(pair[0])? << 4 | hex(pair[1])?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{HttpNineP, NinePBackend, NinePRequestId, NinePRequestStatus};

    fn message(ty: u8, tag: u16, body: &[u8]) -> Vec<u8> {
        let mut result = Vec::from([0, 0, 0, 0, ty]);
        result.extend_from_slice(&tag.to_le_bytes());
        result.extend_from_slice(body);
        let size = u32::try_from(result.len()).unwrap();
        result[..4].copy_from_slice(&size.to_le_bytes());
        result
    }

    fn string(value: &str) -> Vec<u8> {
        let mut result = Vec::from(u16::try_from(value.len()).unwrap().to_le_bytes());
        result.extend_from_slice(value.as_bytes());
        result
    }

    #[test]
    fn initialization_is_lazy_and_remote_reads_resume_after_fetch() {
        let mut fs = HttpNineP::new("https://host/root/", "secret".into());
        let head = fs.next_request().unwrap();
        assert_eq!(head.url, "https://host/root/head");
        fs.complete_request(head.id, b"Version: 1\nRootID: 2a\n".to_vec())
            .unwrap();
        let list = fs.next_request().unwrap();
        assert_eq!(list.url, "https://host/root/files/000000000000002a");
        fs.complete_request(
            list.id,
            b"Version: 1\n100644 1000 1000 3 1.0 hello 2b\n.\n".to_vec(),
        )
        .unwrap();

        let mut version = Vec::from(65_536_u32.to_le_bytes());
        version.extend(string("9P2000.L"));
        assert!(matches!(
            fs.transact(NinePRequestId(1), &message(100, u16::MAX, &version), 4096)
                .unwrap(),
            NinePRequestStatus::Complete(_)
        ));
        let mut attach = Vec::from(1_u32.to_le_bytes());
        attach.extend_from_slice(&u32::MAX.to_le_bytes());
        attach.extend(string(""));
        attach.extend(string(""));
        attach.extend_from_slice(&0_u32.to_le_bytes());
        fs.transact(NinePRequestId(2), &message(104, 1, &attach), 4096)
            .unwrap();
        let mut walk = Vec::from(1_u32.to_le_bytes());
        walk.extend_from_slice(&2_u32.to_le_bytes());
        walk.extend_from_slice(&1_u16.to_le_bytes());
        walk.extend(string("hello"));
        fs.transact(NinePRequestId(3), &message(110, 2, &walk), 4096)
            .unwrap();
        let mut read = Vec::from(2_u32.to_le_bytes());
        read.extend_from_slice(&0_u64.to_le_bytes());
        read.extend_from_slice(&3_u32.to_le_bytes());
        assert_eq!(
            fs.transact(NinePRequestId(4), &message(116, 3, &read), 4096)
                .unwrap(),
            NinePRequestStatus::Pending
        );
        let file = fs.next_request().unwrap();
        assert_eq!(file.url, "https://host/root/files/000000000000002b");
        fs.complete_request(file.id, b"abc".to_vec()).unwrap();
        let NinePRequestStatus::Complete(reply) = fs
            .transact(NinePRequestId(5), &message(116, 3, &read), 4096)
            .unwrap()
        else {
            panic!("read remained pending");
        };
        assert_eq!(&reply[11..], b"abc");
        assert!(fs.child(0, ".fscmd_pwd").is_ok());
        fs.command(9, b"pbkdf2 70617373776f7264 73616c74 2 16")
            .unwrap();
        assert_eq!(
            fs.command_replies.get(&9).unwrap(),
            &[
                0xae, 0x4d, 0x0c, 0x95, 0xaf, 0x6b, 0x46, 0xd3, 0x2d, 0x0a, 0xdf, 0xf9, 0x28, 0xf0,
                0x6d, 0xd0
            ]
        );
    }
}
