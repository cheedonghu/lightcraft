use super::*;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lc-ux02-store-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn bounded_and_range(store: &mut dyn Store) {
    store.write_atomic("bytes", b"abcdef").unwrap();
    assert_eq!(store.read_limited("bytes", 6).unwrap().unwrap(), b"abcdef");
    assert_eq!(store.read_limited("bytes", 5).unwrap_err().kind(), io::ErrorKind::InvalidData);
    assert_eq!(store.read_limited("missing", 1).unwrap(), None);
    assert_eq!(store.read_range("bytes", 2, 3).unwrap(), b"cde");
    assert_eq!(store.read_range("bytes", 6, 0).unwrap(), b"");
    assert_eq!(store.read_range("bytes", 6, 1).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    assert_eq!(store.read_range("bytes", u64::MAX, 2).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    assert_eq!(store.read_range("missing", 0, 1).unwrap_err().kind(), io::ErrorKind::NotFound);
    store.remove("bytes").unwrap();
    store.remove("bytes").unwrap();
    store.remove("missing/child").unwrap();
    assert!(store.read("bytes").unwrap().is_none());
}

#[test]
fn memory_bounded_reads_ranges_and_deletion() {
    bounded_and_range(&mut MemStore::new());
}

#[test]
fn native_bounded_reads_ranges_and_deletion() {
    let dir = temp("bounded");
    bounded_and_range(&mut FsStore::open(&dir).unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_nested_atomic_writes_survive_reopen_and_abort() {
    let dir = temp("nested");
    let mut store = FsStore::open(&dir).unwrap();
    store.write_atomic("companion/photos/1/head", b"old").unwrap();
    let err = store.write_atomic_with("companion/photos/1/head", &mut |writer| {
        writer.write_all(b"partial")?;
        Err(io::Error::other("injected stream failure"))
    });
    assert!(err.is_err());
    drop(store);
    let mut store = FsStore::open(&dir).unwrap();
    assert_eq!(store.read("companion/photos/1/head").unwrap().unwrap(), b"old");
    assert_eq!(std::fs::read_dir(dir.join("companion/photos/1")).unwrap().count(), 1);
    store.write_atomic_with("companion/photos/2/head", &mut |writer| writer.write_all(b"new")).unwrap();
    store.remove("companion/photos/1/head").unwrap();
    drop(store);
    let mut store = FsStore::open(&dir).unwrap();
    assert!(store.read("companion/photos/1/head").unwrap().is_none());
    assert_eq!(store.read("companion/photos/2/head").unwrap().unwrap(), b"new");
    std::fs::remove_dir_all(dir).unwrap();
}

fn reject_paths(store: &mut dyn Store) {
    for name in ["", "../escape", "/absolute", "a/../escape", "a/./x", "a//x", "a\\x", "C:/escape", "a/", "nul\0x"] {
        assert_eq!(store.write_atomic(name, b"bad").unwrap_err().kind(), io::ErrorKind::InvalidInput, "{name:?}");
        assert_eq!(store.read_limited(name, 1).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(store.read_range(name, 0, 1).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(store.remove(name).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }
}

#[test]
fn memory_rejects_paths_outside_controlled_namespace() {
    reject_paths(&mut MemStore::new());
}

#[test]
fn native_rejects_paths_outside_controlled_namespace() {
    let dir = temp("paths");
    reject_paths(&mut FsStore::open(&dir).unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn native_rejects_symlink_directory_escape() {
    let dir = temp("symlink");
    let outside = temp("outside");
    let mut store = FsStore::open(&dir).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep"), b"safe").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();
    assert!(store.read_limited("link/keep", 10).is_err());
    assert!(store.write_atomic("link/keep", b"bad").is_err());
    assert!(store.remove("link/keep").is_err());
    assert_eq!(std::fs::read(outside.join("keep")).unwrap(), b"safe");
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[test]
fn older_store_implementations_report_unsupported_without_unbounded_read() {
    struct OldStore {
        reads: usize,
    }
    impl Store for OldStore {
        fn read(&mut self, _: &str) -> io::Result<Option<Vec<u8>>> {
            self.reads += 1;
            Ok(Some(b"legacy".to_vec()))
        }
        fn write_atomic(&mut self, _: &str, _: &[u8]) -> io::Result<()> {
            Ok(())
        }
        fn append(&mut self, _: &str, _: &[u8]) -> io::Result<()> {
            Ok(())
        }
        fn truncate(&mut self, _: &str, _: u64) -> io::Result<()> {
            Ok(())
        }
        fn describe(&self) -> String {
            "legacy".into()
        }
    }
    let mut old = OldStore { reads: 0 };
    assert_eq!(old.read_limited("bytes", 2).unwrap_err().kind(), io::ErrorKind::Unsupported);
    assert_eq!(old.read_range("bytes", 0, 2).unwrap_err().kind(), io::ErrorKind::Unsupported);
    assert_eq!(old.remove("bytes").unwrap_err().kind(), io::ErrorKind::Unsupported);
    assert_eq!(old.reads, 0);
    assert_eq!(old.read("bytes").unwrap().unwrap(), b"legacy");
}

#[test]
fn native_sparse_file_and_huge_ranges_fail_before_allocating() {
    let dir = temp("sparse");
    let mut store = FsStore::open(&dir).unwrap();
    std::fs::File::create(dir.join("huge")).unwrap().set_len(1 << 40).unwrap();
    assert_eq!(store.read_limited("huge", 128 * 1024).unwrap_err().kind(), io::ErrorKind::InvalidData);
    assert_eq!(store.read_range("huge", 1 << 40, 1).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    assert_eq!(store.read_limited("huge", u64::MAX).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_delete_closes_cached_appender_before_a_new_append() {
    let dir = temp("appender");
    let mut store = FsStore::open(&dir).unwrap();
    store.append("catalog.log", b"old").unwrap();
    store.remove("catalog.log").unwrap();
    store.append("catalog.log", b"new").unwrap();
    assert_eq!(store.read("catalog.log").unwrap().unwrap(), b"new");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn real_read_stops_at_limit_when_data_exceeds_its_initial_size() {
    use io::Read;
    struct Growing {
        consumed: usize,
    }
    impl Read for Growing {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            bytes.fill(b'x');
            self.consumed += bytes.len();
            Ok(bytes.len())
        }
    }
    let mut reader = Growing { consumed: 0 };
    assert_eq!(read_bytes((&mut reader).take(9)).unwrap().len(), 9);
    assert_eq!(reader.consumed, 9, "only the limit and one overflow byte reach the underlying I/O");
}

#[test]
fn native_write_failure_preserves_previous_committed_bytes() {
    let dir = temp("fault");
    let mut store = FsStore::open(&dir).unwrap();
    store.write_atomic("companion/photos/1/head", b"committed").unwrap();
    {
        let _fault = crate::safe_file::fail_writes_after(3);
        assert!(store.write_atomic("companion/photos/1/head", b"uncommitted").is_err());
    }
    assert_eq!(store.read("companion/photos/1/head").unwrap().unwrap(), b"committed");
    assert_eq!(std::fs::read_dir(dir.join("companion/photos/1")).unwrap().count(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}
