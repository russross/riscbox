//! Bounded little-endian messages are parsed completely before dispatch.

use std::str::from_utf8;

use super::{ENOSPC, EPROTO};
use crate::ninep::{Inode, InodeKind};

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(super) fn take(&mut self, size: usize) -> Result<&'a [u8], u32> {
        let end = self.offset.checked_add(size).ok_or(EPROTO)?;
        let bytes = self.bytes.get(self.offset..end).ok_or(EPROTO)?;
        self.offset = end;
        Ok(bytes)
    }

    pub(super) fn u8(&mut self) -> Result<u8, u32> {
        Ok(self.take(1)?[0])
    }
    pub(super) fn u16(&mut self) -> Result<u16, u32> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }
    pub(super) fn u32(&mut self) -> Result<u32, u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    pub(super) fn u64(&mut self) -> Result<u64, u32> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    pub(super) fn string(&mut self) -> Result<&'a str, u32> {
        let size = usize::from(self.u16()?);
        let value = from_utf8(self.take(size)?).map_err(|_| EPROTO)?;
        if value.contains('\0') {
            return Err(EPROTO);
        }
        Ok(value)
    }

    pub(super) fn done(&self) -> Result<(), u32> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(EPROTO)
        }
    }
}

pub(super) struct Writer {
    pub(super) bytes: Vec<u8>,
    capacity: usize,
}

impl Writer {
    pub(super) fn remaining(&self) -> usize {
        self.capacity - self.bytes.len()
    }
    pub(super) fn new(kind: u8, tag: u16, capacity: usize) -> Result<Self, u32> {
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

    pub(super) fn append(&mut self, bytes: &[u8]) -> Result<(), u32> {
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

    pub(super) fn u8(&mut self, value: u8) -> Result<(), u32> {
        self.append(&[value])
    }
    pub(super) fn u16(&mut self, value: u16) -> Result<(), u32> {
        self.append(&value.to_le_bytes())
    }
    pub(super) fn u32(&mut self, value: u32) -> Result<(), u32> {
        self.append(&value.to_le_bytes())
    }
    pub(super) fn u64(&mut self, value: u64) -> Result<(), u32> {
        self.append(&value.to_le_bytes())
    }

    pub(super) fn string(&mut self, value: &str) -> Result<(), u32> {
        self.u16(u16::try_from(value.len()).map_err(|_| ENOSPC)?)?;
        self.append(value.as_bytes())
    }

    pub(super) fn qid(&mut self, inode: &Inode) -> Result<(), u32> {
        self.u8(match inode.kind {
            InodeKind::Directory { .. } => 0x80,
            InodeKind::Symlink(_) => 2,
            InodeKind::File(_) => 0,
        })?;
        self.u32(inode.version)?;
        self.u64(inode.id.0)
    }

    pub(super) fn finish(mut self) -> Vec<u8> {
        let length = u32::try_from(self.bytes.len()).expect("bounded message size");
        self.bytes[..4].copy_from_slice(&length.to_le_bytes());
        self.bytes
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Attributes {
    pub(super) mask: u32,
    pub(super) mode: u32,
    pub(super) uid: u32,
    pub(super) gid: u32,
    pub(super) size: u64,
    pub(super) atime: u64,
    pub(super) atime_ns: u64,
    pub(super) mtime: u64,
    pub(super) mtime_ns: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Lock<'a> {
    pub(super) fid: u32,
    pub(super) kind: u8,
    pub(super) flags: u32,
    pub(super) start: u64,
    pub(super) length: u64,
    pub(super) process: u32,
    pub(super) client: &'a str,
}

#[derive(Clone, Copy)]
pub(super) struct Creation<'a> {
    pub(super) name: &'a str,
    pub(super) flags: u32,
    pub(super) mode: u32,
    pub(super) gid: u32,
}

pub(super) enum Request<'a> {
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
    pub(super) fn parse(kind: u8, bytes: &'a [u8]) -> Result<Self, u32> {
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
                    return Err(super::EINVAL);
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

    pub(super) const fn minimum_reply(&self) -> usize {
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
