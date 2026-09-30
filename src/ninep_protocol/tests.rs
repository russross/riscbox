//! Wire-level regressions use independent packet builders and observable replies.

use super::{Completion, NinePSession, Outcome, ProtocolError, RequestId, Submission};
use crate::ninep::{
    ChangeSource, FileRead, Filesystem, FilesystemError, Limits, LoadStart, SourceId,
};

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
    next: u64,
    tag: u16,
}
impl Client {
    fn mount(fs: &mut Filesystem) -> Self {
        let mut client = Self {
            session: NinePSession::new(fs).unwrap(),
            next: 1,
            tag: 1,
        };
        let reply = client.call_tag(fs, 100, u16::MAX, version(8192, "9P2000.L"), 8192);
        assert_eq!(reply[4], 101);
        assert_eq!(client.call(fs, 104, attach(1))[4], 105);
        client
    }
    fn submit(
        &mut self,
        fs: &mut Filesystem,
        kind: u8,
        tag: u16,
        body: &Body,
        capacity: usize,
    ) -> Result<Submission, ProtocolError> {
        self.submit_packet(fs, &packet(kind, tag, body), capacity)
    }
    fn submit_packet(
        &mut self,
        fs: &mut Filesystem,
        bytes: &[u8],
        capacity: usize,
    ) -> Result<Submission, ProtocolError> {
        let id = RequestId(self.next);
        self.next += 1;
        self.session.submit(fs, id, bytes, capacity)
    }
    fn call_tag(
        &mut self,
        fs: &mut Filesystem,
        kind: u8,
        tag: u16,
        body: Body,
        capacity: usize,
    ) -> Vec<u8> {
        let Submission::Immediate(reply) = self
            .submit_packet(fs, &body.into_packet(kind, tag), capacity)
            .unwrap()
        else {
            panic!("unexpected pending operation {kind}");
        };
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
    fn pending(
        &mut self,
        fs: &mut Filesystem,
        kind: u8,
        tag: u16,
        body: Body,
    ) -> (RequestId, LoadStart) {
        let Submission::Pending(load) = self
            .submit_packet(fs, &body.into_packet(kind, tag), 8192)
            .unwrap()
        else {
            panic!("expected lazy operation");
        };
        (RequestId(self.next - 1), load)
    }
}
fn filesystem() -> Filesystem {
    Filesystem::new(Limits::default(), 100)
}
fn completion(session: &mut NinePSession, id: RequestId, kind: u8) -> Vec<u8> {
    let Completion {
        request,
        outcome: Outcome::Reply(bytes),
    } = session.next_completion().unwrap()
    else {
        panic!("expected reply");
    };
    assert_eq!(request, id);
    assert_eq!(bytes[4], kind);
    bytes
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
    let Submission::Immediate(reply) = c
        .session
        .submit(&mut fs, RequestId(500), &bytes, 11)
        .unwrap()
    else {
        panic!();
    };
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
        c.session.submit(&mut fs, RequestId(501), &[0; 6], 100),
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
    assert_eq!(
        fs.read_file("file"),
        Ok(FileRead::Resident(b"\0\0xyz".to_vec()))
    );
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
    assert_eq!(fs.read_file("file"), Ok(FileRead::Resident(Vec::new())));
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
    assert_eq!(
        fs.read_file("file"),
        Ok(FileRead::Resident(b"abc\0".to_vec()))
    );
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
    assert_eq!(
        fs.read_file("restored"),
        Ok(FileRead::Resident(b"open".to_vec()))
    );
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

// A flush retires only its request, while another endpoint may still need the
// shared load. Tag reuse after Rflush must not inherit the flushed continuation.
#[test]
fn shared_lazy_reads_flush_duplicate_tags_and_tag_reuse_are_coherent() {
    let mut fs = filesystem();
    fs.add_lazy_file("file", 3, SourceId(1)).unwrap();
    let mut a = Client::mount(&mut fs);
    let mut b = Client::mount(&mut fs);
    a.open(&mut fs, 2, "file", 0);
    b.open(&mut fs, 2, "file", 0);
    let (old, started) = a.pending(&mut fs, 116, 100, read(2, 0, 3));
    assert!(matches!(started, LoadStart::Started(_)));
    let (other, joined) = b.pending(&mut fs, 116, 100, read(2, 0, 3));
    assert_eq!(joined, LoadStart::Joined(started.ticket()));
    assert_eq!(
        a.submit(&mut fs, 116, 100, &read(2, 0, 1), 8192),
        Err(ProtocolError::DuplicateTag)
    );
    assert_eq!(
        a.session
            .submit(&mut fs, old, &packet(116, 101, &read(2, 0, 1)), 8192),
        Err(ProtocolError::DuplicateRequestId)
    );
    assert_eq!(
        a.call_tag(&mut fs, 108, 102, Body::default().u16(100), 7)[4],
        109
    );
    assert_eq!(
        a.session.next_completion(),
        Some(Completion {
            request: old,
            outcome: Outcome::Suppressed
        })
    );
    let (reused, joined) = a.pending(&mut fs, 116, 100, read(2, 1, 2));
    assert_eq!(joined, LoadStart::Joined(started.ticket()));
    fs.complete_load(started.ticket(), b"abc".to_vec()).unwrap();
    a.session.poll(&mut fs).unwrap();
    b.session.poll(&mut fs).unwrap();
    assert_eq!(&completion(&mut a.session, reused, 117)[11..], b"bc");
    assert_eq!(&completion(&mut b.session, other, 117)[11..], b"abc");
    assert!(a.session.next_completion().is_none());
    assert_eq!(a.call(&mut fs, 108, Body::default().u16(100))[4], 109);
}

#[test]
fn pending_write_survives_clunk_unlink_and_fid_reuse_without_redirection() {
    let mut fs = filesystem();
    let old_inode = fs.add_lazy_file("file", 3, SourceId(1)).unwrap();
    let mut c = Client::mount(&mut fs);
    c.open(&mut fs, 2, "file", 2);
    c.open(&mut fs, 3, "file", 0);
    let (request, load) = c.pending(&mut fs, 118, 100, write(2, 1, b"X"));
    c.call(&mut fs, 120, Body::default().u32(2));
    c.call(&mut fs, 76, Body::default().u32(1).string("file").u32(0));
    fs.write_file("file", b"new").unwrap();
    c.open(&mut fs, 2, "file", 2);
    fs.complete_load(load.ticket(), b"abc".to_vec()).unwrap();
    c.session.poll(&mut fs).unwrap();
    assert_eq!(at32(&completion(&mut c.session, request, 119), 7), 1);
    assert_eq!(&c.call(&mut fs, 116, read(3, 0, 3))[11..], b"aXc");
    assert_eq!(
        fs.read_file("file"),
        Ok(FileRead::Resident(b"new".to_vec()))
    );
    c.call(&mut fs, 120, Body::default().u32(3));
    assert!(fs.inode(old_inode).is_none());
    assert_eq!(fs.space_usage().used_bytes, 3);
}

#[test]
fn host_write_wins_over_a_load_and_pending_reads_observe_the_current_bytes() {
    let mut fs = filesystem();
    fs.add_lazy_file("file", 3, SourceId(1)).unwrap();
    let mut c = Client::mount(&mut fs);
    c.open(&mut fs, 2, "file", 0);
    let (request, load) = c.pending(&mut fs, 116, 100, read(2, 0, 3));
    fs.write_file("file", b"new").unwrap();
    c.session.poll(&mut fs).unwrap();
    assert_eq!(&completion(&mut c.session, request, 117)[11..], b"new");
    assert_eq!(
        fs.complete_load(load.ticket(), b"old".to_vec()),
        Err(FilesystemError::StaleLoad)
    );
}

#[test]
fn failed_lazy_setattr_does_not_change_metadata_and_current_time_is_commit_time() {
    let mut fs = filesystem();
    let id = fs.add_lazy_file("file", 3, SourceId(1)).unwrap();
    let mut c = Client::mount(&mut fs);
    c.open(&mut fs, 2, "file", 2);
    let (request, load) = c.pending(&mut fs, 26, 100, attributes(2, 1 | 8 | 0x20, 4, 0, 0));
    assert_eq!(fs.inode(id).unwrap().mode, 0o644);
    fs.fail_load(load.ticket()).unwrap();
    fs.retry_load("file").unwrap();
    c.session.poll(&mut fs).unwrap();
    error(&completion(&mut c.session, request, 7), 5);
    assert_eq!(fs.inode(id).unwrap().mode, 0o644);
    let (request, load) = c.pending(&mut fs, 26, 100, attributes(2, 1 | 8 | 0x20, 4, 0, 0));
    fs.set_time(300);
    fs.complete_load(load.ticket(), b"abc".to_vec()).unwrap();
    c.session.poll(&mut fs).unwrap();
    assert_eq!(completion(&mut c.session, request, 27).len(), 7);
    assert_eq!(
        (fs.inode(id).unwrap().mode, fs.inode(id).unwrap().mtime),
        (0o600, 300)
    );
    assert_eq!(
        fs.read_file("file"),
        Ok(FileRead::Resident(b"abc\0".to_vec()))
    );
}

#[test]
fn zero_truncation_and_eof_reads_do_not_start_lazy_loads() {
    let mut fs = filesystem();
    fs.add_lazy_file("file", 3, SourceId(1)).unwrap();
    let mut c = Client::mount(&mut fs);
    c.open(&mut fs, 2, "file", 2);
    assert_eq!(at32(&c.call(&mut fs, 116, read(2, 3, 1)), 7), 0);
    assert_eq!(at32(&c.call(&mut fs, 116, read(2, 0, 0)), 7), 0);
    assert_eq!(c.call(&mut fs, 26, attributes(2, 8, 0, 0, 0))[4], 27);
    assert_eq!(fs.read_file("file"), Ok(FileRead::Resident(Vec::new())));
}

#[test]
fn version_reset_retires_pending_requests_fids_and_locks_but_retains_tree() {
    let mut fs = filesystem();
    fs.write_file("resident", b"x").unwrap();
    fs.add_lazy_file("lazy", 1, SourceId(1)).unwrap();
    let mut a = Client::mount(&mut fs);
    let mut b = Client::mount(&mut fs);
    a.open(&mut fs, 2, "resident", 2);
    b.open(&mut fs, 2, "resident", 2);
    a.open(&mut fs, 3, "lazy", 0);
    assert_eq!(a.call(&mut fs, 52, lock(2, 1, Some(0), 0, 0, 10))[7], 0);
    let (request, load) = a.pending(&mut fs, 116, 100, read(3, 0, 1));
    error(
        &a.call_tag(&mut fs, 100, u16::MAX, version(0, "9P2000.L"), 100),
        22,
    );
    assert!(a.session.next_completion().is_none());
    a.call_tag(&mut fs, 100, u16::MAX, version(4096, "9P2000.L"), 100);
    assert_eq!(
        a.session.next_completion(),
        Some(Completion {
            request,
            outcome: Outcome::Suppressed
        })
    );
    error(&a.call(&mut fs, 116, read(2, 0, 1)), 9);
    assert_eq!(b.call(&mut fs, 52, lock(2, 1, Some(0), 0, 0, 20))[7], 0);
    fs.complete_load(load.ticket(), b"y".to_vec()).unwrap();
    a.session.poll(&mut fs).unwrap();
    assert!(a.session.next_completion().is_none());
    assert_eq!(fs.list_files(), vec!["lazy", "resident"]);
}

#[test]
fn namespace_replacement_returns_stale_errors_and_rejects_old_fids_without_panics() {
    let mut fs = filesystem();
    fs.add_lazy_file("file", 1, SourceId(1)).unwrap();
    let mut c = Client::mount(&mut fs);
    c.open(&mut fs, 2, "file", 0);
    let (request, _) = c.pending(&mut fs, 116, 100, read(2, 0, 1));
    fs.reset().unwrap();
    c.session.poll(&mut fs).unwrap();
    error(&completion(&mut c.session, request, 7), 116);
    error(&c.call(&mut fs, 110, walk(1, 3, &["."])), 9);
    assert_eq!(c.call(&mut fs, 104, attach(1))[4], 105);
    let mut other = filesystem();
    assert_eq!(
        c.session
            .submit(&mut other, RequestId(999), &packet(104, 1, &attach(2)), 100),
        Err(ProtocolError::WrongFilesystem)
    );
    c.session.close(&mut fs).unwrap();
    c.session.close(&mut fs).unwrap();
    assert_eq!(
        c.session
            .submit(&mut fs, RequestId(999), &packet(104, 1, &attach(2)), 100),
        Err(ProtocolError::Closed)
    );
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
fn device_reset_discards_completions_and_pending_protocol_state_only() {
    let mut fs = filesystem();
    fs.add_lazy_file("file", 1, SourceId(1)).unwrap();
    let mut c = Client::mount(&mut fs);
    c.open(&mut fs, 2, "file", 0);
    let (_, load) = c.pending(&mut fs, 116, 100, read(2, 0, 1));
    c.session.reset(&mut fs).unwrap();
    fs.complete_load(load.ticket(), b"x".to_vec()).unwrap();
    c.session.poll(&mut fs).unwrap();
    assert!(c.session.next_completion().is_none());
    assert_eq!(fs.read_file("file"), Ok(FileRead::Resident(b"x".to_vec())));
    error(&c.call(&mut fs, 104, attach(1)), 71);
    c.call_tag(&mut fs, 100, u16::MAX, version(8192, "9P2000.L"), 100);
    assert_eq!(c.call(&mut fs, 104, attach(1))[4], 105);
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
    assert_eq!(fs.read_file("file"), Ok(FileRead::Resident(Vec::new())));
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
fn malformed_flush_preserves_pending_state_and_queued_replies_reserve_tags() {
    let mut fs = filesystem();
    fs.add_lazy_file("file", 1, SourceId(1)).unwrap();
    let mut c = Client::mount(&mut fs);
    c.open(&mut fs, 2, "file", 0);
    let (request, load) = c.pending(&mut fs, 116, 100, read(2, 0, 1));
    let bad = Body::default().u16(100).bytes(&[0]);
    assert_eq!(
        c.submit(&mut fs, 108, 101, &bad, 100),
        Err(ProtocolError::Malformed)
    );
    assert!(c.session.next_completion().is_none());
    fs.complete_load(load.ticket(), b"x".to_vec()).unwrap();
    c.session.poll(&mut fs).unwrap();
    assert_eq!(
        c.submit(&mut fs, 116, 100, &read(2, 0, 1), 100),
        Err(ProtocolError::DuplicateTag)
    );
    assert_eq!(&completion(&mut c.session, request, 117)[11..], b"x");
    assert_eq!(
        &c.call_tag(&mut fs, 116, 100, read(2, 0, 1), 100)[11..],
        b"x"
    );
    assert_eq!(
        c.call_tag(&mut fs, 108, 101, Body::default().u16(100), 7)[4],
        109
    );
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
    assert_eq!(
        fs.read_file("π.c"),
        Ok(FileRead::Resident("λ".as_bytes().to_vec()))
    );
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
