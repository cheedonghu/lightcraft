//! Byte storage for the catalog files (`catalog.snap`, `catalog.log`).
//!
//! [`FsStore`] is the native implementation (a library directory); [`MemStore`] keeps files in
//! memory (tests, ephemeral sessions) and lets tests simulate crashes by editing the bytes. A web
//! host can implement [`Store`] over OPFS.

pub mod source;
use source::{SourceIdentity, SourceSnapshot};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Named files relative to a controlled storage root. Native and memory stores reject
/// absolute paths, traversal and symlink references. Existing journal operations stay available;
/// new bounded operations default to explicit `Unsupported` errors for custom stores.
pub trait Store: Send {
    /// The whole file, or `None` if it doesn't exist.
    fn read(&mut self, name: &str) -> io::Result<Option<Vec<u8>>>;
    /// Read at most `max_bytes`; oversized files return `InvalidData` without unbounded I/O.
    /// Unsupported stores must not fall back to a whole-file read.
    fn read_limited(&mut self, _name: &str, _max_bytes: u64) -> io::Result<Option<Vec<u8>>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "bounded reads are unsupported by this store"))
    }
    /// Read exactly `length` bytes at `offset`. Missing files return `NotFound`, short ranges
    /// return `UnexpectedEof`, and arithmetic overflow returns `InvalidInput`.
    fn read_range(&mut self, _name: &str, _offset: u64, _length: usize) -> io::Result<Vec<u8>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "range reads are unsupported by this store"))
    }
    /// Capture a bounded reader tied to this exact source; changes are errors,
    /// never mixed reads. Native and memory adapters support it; custom adapters
    /// must opt in rather than using a whole-file fallback.
    fn source_snapshot(&mut self, _name: &str) -> io::Result<Option<Box<dyn SourceSnapshot>>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "source snapshots are unsupported by this store"))
    }
    /// Stream a verified source into a durable atomic destination. Unlike the
    /// general write_atomic_with default this has no whole-source fallback.
    fn copy_source_atomic(&mut self, _name: &str, _source: &mut dyn SourceSnapshot) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "streamed source copying is unsupported by this store"))
    }
    /// Replace the original source only while its identity still matches.
    /// Callers coordinate all ordinary writers across verification/publication.
    fn replace_source_atomic(&mut self, _name: &str, _expected: &SourceIdentity, _data: &[u8]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "verified source replacement is unsupported by this store"))
    }
    /// Conditional source publication with caller-owned revocable authority.
    /// Expensive fresh identity proofs may run on an owned worker; adapters
    /// recheck authority immediately before the atomic publication. Unsupported
    /// adapters must not weaken this to a pre-check plus an unchecked writer.
    fn replace_source_atomic_authorized(
        &mut self,
        _name: &str,
        _expected: &SourceIdentity,
        _data: &[u8],
        _authorized: &dyn Fn() -> bool,
    ) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "revocable source publication is unsupported by this store"))
    }
    /// Durably remove a file. Removing a missing file succeeds.
    fn remove(&mut self, _name: &str) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "file removal is unsupported by this store"))
    }
    /// Replace a file atomically: after a crash either the old or the new content is visible,
    /// never a mix (native: write temp + fsync + rename + fsync directory).
    fn write_atomic(&mut self, name: &str, data: &[u8]) -> io::Result<()>;
    /// [`Store::write_atomic`] with streamed content: `fill` writes the new content into the
    /// given writer (an error aborts and leaves the old file). Same durability: the file is
    /// replaced only once all of it is written (native: and fsynced). Returns its length.
    ///
    /// The default buffers the content in memory; [`FsStore`] streams it into the temp file, so
    /// a large snapshot is never held in memory as a whole.
    fn write_atomic_with(&mut self, name: &str, fill: &mut dyn FnMut(&mut dyn io::Write) -> io::Result<()>) -> io::Result<u64> {
        let mut buf = Vec::new();
        fill(&mut buf)?;
        self.write_atomic(name, &buf)?;
        Ok(buf.len() as u64)
    }
    /// Append and make durable (fsync) before returning. On an error, part of `data` may have
    /// been written (a full disk, a dropped network share): the journal cuts the file back to its
    /// previous length with [`Store::truncate`] before appending again.
    fn append(&mut self, name: &str, data: &[u8]) -> io::Result<()>;
    /// Cut a file to `len` bytes (drop a torn tail before appending again). A missing file is
    /// left missing.
    fn truncate(&mut self, name: &str, len: u64) -> io::Result<()>;
    /// Human-readable location (diagnostics).
    fn describe(&self) -> String;
    /// Stable process-local identity for reader retention across close/reopen.
    /// Native aliases must identify the same physical directory; wrappers forward
    /// their adapter's identity. Unknown adapters cannot offer retained readers.
    fn retention_namespace(&self) -> Option<String> {
        None
    }
    /// Require a shared process-wide physical-reclamation pause while any
    /// conservative reader lives. Ownership/debt still uses retention_namespace;
    /// wrappers must forward both properties, never use a global owner identity.
    fn conservative_retention(&self) -> bool {
        false
    }
    /// An owned adapter for potentially large source migrations. Custom stores
    /// may reuse their background writer; memory migrations do not change the
    /// journal compaction background preference.
    fn migration_writer(&self) -> Option<Box<dyn Store>> {
        self.background_writer()
    }
    /// Another handle on the same files that a worker thread can write `catalog.snap` through
    /// while this one keeps appending to the log (background compaction). `None` (the default):
    /// snapshots are written synchronously.
    fn background_writer(&self) -> Option<Box<dyn Store>> {
        None
    }
}

/// Files in a directory.
pub struct FsStore {
    dir: PathBuf,
    /// Open append handle, kept between appends. Every removal/rewrite/truncate drops it:
    /// native aliases can identify the same file even when their keys differ.
    appender: Option<(String, std::fs::File)>,
}

impl FsStore {
    /// Use `dir` (created if missing).
    pub fn open(dir: impl AsRef<Path>) -> io::Result<FsStore> {
        std::fs::create_dir_all(dir.as_ref())?;
        Ok(FsStore { dir: dir.as_ref().to_path_buf(), appender: None })
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }
    fn path(&self, name: &str) -> io::Result<PathBuf> {
        validate_name(name)?;
        let mut path = self.dir.clone();
        for part in name.split('/') {
            path.push(part);
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, "storage paths may not contain symlinks"));
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(path)
    }
    fn prepare_parent(&self, name: &str) -> io::Result<PathBuf> {
        let path = self.path(name)?;
        let mut parent = self.dir.clone();
        let mut parts = name.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                break;
            }
            let previous = parent.clone();
            parent.push(part);
            match std::fs::create_dir(&parent) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && parent.is_dir() => {}
                Err(e) => return Err(e),
            }
            // Also persist an existing parent: a previous attempt might have failed to sync it.
            sync_directory(&previous)?;
        }
        Ok(path)
    }
}

impl Store for FsStore {
    fn source_snapshot(&mut self, name: &str) -> io::Result<Option<Box<dyn SourceSnapshot>>> {
        source::native_snapshot(self.path(name)?)
    }
    fn copy_source_atomic(&mut self, name: &str, source: &mut dyn SourceSnapshot) -> io::Result<()> {
        self.write_atomic_with(name, &mut |writer| source::copy(source, writer)).map(|_| ())
    }
    fn replace_source_atomic(&mut self, name: &str, expected: &SourceIdentity, data: &[u8]) -> io::Result<()> {
        self.replace_source_atomic_authorized(name, expected, data, &|| true)
    }
    fn replace_source_atomic_authorized(
        &mut self,
        name: &str,
        expected: &SourceIdentity,
        data: &[u8],
        authorized: &dyn Fn() -> bool,
    ) -> io::Result<()> {
        source::authority(authorized)?;
        let path = self.path(name)?;
        let mut check = || {
            source::check_native(&path, expected)?;
            source::authority(authorized)
        };
        crate::safe_file::write_atomic_checked(&path, data, &mut check)?;
        sync_directory(path.parent().unwrap_or(&self.dir))
    }
    fn read(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        match std::fs::read(self.path(name)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn read_limited(&mut self, name: &str, max_bytes: u64) -> io::Result<Option<Vec<u8>>> {
        use io::Read;
        let path = self.path(name)?;
        let limit = max_bytes.checked_add(1).ok_or_else(|| invalid("bounded read limit is too large"))?;
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if file.metadata()?.len() > max_bytes {
            return Err(oversized(max_bytes));
        }
        // `take` bounds real reads even if the file grows after the metadata check.
        let bytes = read_bytes(file.take(limit))?;
        if bytes.len() as u64 > max_bytes {
            return Err(oversized(max_bytes));
        }
        Ok(Some(bytes))
    }

    fn read_range(&mut self, name: &str, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        use io::{Read, Seek};
        let path = self.path(name)?;
        let end = range_end(offset, length)?;
        let mut file = std::fs::File::open(path)?;
        if end > file.metadata()?.len() {
            return Err(short_range());
        }
        file.seek(io::SeekFrom::Start(offset))?;
        let bytes = read_bytes(file.take(length as u64))?;
        if bytes.len() != length {
            return Err(short_range());
        }
        Ok(bytes)
    }

    fn remove(&mut self, name: &str) -> io::Result<()> {
        let path = self.path(name)?;
        self.appender = None;
        match std::fs::remove_file(&path) {
            Ok(()) => sync_directory(path.parent().unwrap_or(&self.dir)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Retry the parent sync if a prior delete succeeded but its sync failed.
                let parent = path.parent().unwrap_or(&self.dir);
                if parent.is_dir() { sync_directory(parent) } else { Ok(()) }
            }
            Err(e) => Err(e),
        }
    }

    fn write_atomic(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        self.write_atomic_with(name, &mut |writer| writer.write_all(data)).map(|_| ())
    }

    fn write_atomic_with(&mut self, name: &str, fill: &mut dyn FnMut(&mut dyn io::Write) -> io::Result<()>) -> io::Result<u64> {
        let path = self.prepare_parent(name)?;
        self.appender = None;
        // Preserve recovery from the old fixed temp name. Unlinking a leftover symlink removes
        // only that link; it never opens its target. `.tmp` is reserved by the storage writer.
        let legacy_tmp = self.dir.join(format!("{name}.tmp"));
        match std::fs::remove_file(legacy_tmp) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        // The shared durable writer uses unique, exclusively created temp files and removes
        // them on a failed fill. Report directory sync errors instead of silently accepting them.
        let n = crate::safe_file::write_atomic_with(&path, fill)?;
        sync_directory(path.parent().unwrap_or(&self.dir))?;
        Ok(n)
    }

    fn append(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        use std::io::Write;
        validate_name(name)?;
        if self.appender.as_ref().is_none_or(|(n, _)| n != name) {
            let path = self.prepare_parent(name)?;
            let f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
            // An earlier attempt can create the file and then fail this sync. Retry it on
            // every new handle, even when the name already exists, before writing any bytes.
            sync_directory(path.parent().unwrap_or(&self.dir))?;
            self.appender = Some((name.to_string(), f));
        }
        let Some((_, f)) = self.appender.as_mut() else { return Err(io::Error::other("appender not open")) };
        let written = f.write_all(data).and_then(|()| f.sync_data());
        if written.is_err() {
            // The handle may be dead (a network share that reconnected, a removed volume): the next
            // attempt reopens the file. Part of `data` may be in the file; the caller cuts it back
            // ([`Store::truncate`]) before appending again.
            self.appender = None;
        }
        written
    }

    fn truncate(&mut self, name: &str, len: u64) -> io::Result<()> {
        self.appender = None;
        let f = match std::fs::OpenOptions::new().write(true).open(self.path(name)?) {
            Ok(f) => f,
            // nothing to cut (an append that failed before creating the file)
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        f.set_len(len)?;
        f.sync_all()
    }

    fn describe(&self) -> String {
        self.dir.display().to_string()
    }

    fn retention_namespace(&self) -> Option<String> {
        let metadata = std::fs::metadata(&self.dir).ok()?;
        if !metadata.is_dir() {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(format!("native-directory:{}:{}", metadata.dev(), metadata.ino()))
        }
        #[cfg(windows)]
        {
            // Safe directory-handle identity routes only this library's debt,
            // including case/symlink aliases. Unavailable/partial IDs protect GC.
            let handle = winapi_util::Handle::from_path_any(&self.dir).ok()?;
            let info = winapi_util::file::information(&handle).ok()?;
            let (volume, index) = (info.volume_serial_number(), info.file_index());
            (volume != 0 && index != 0).then(|| format!("native-windows-directory:{volume}:{index}"))
        }
        #[cfg(not(any(unix, windows)))]
        {
            None
        }
    }
    fn conservative_retention(&self) -> bool {
        cfg!(windows)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn background_writer(&self) -> Option<Box<dyn Store>> {
        Some(Box::new(FsStore { dir: self.dir.clone(), appender: None }))
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.contains(['\\', ':', '\0']) || name.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
        return Err(invalid("storage name must be a relative path without empty or traversal components"));
    }
    Ok(())
}

fn oversized(max_bytes: u64) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("storage file exceeds the {max_bytes}-byte read limit"))
}

fn range_end(offset: u64, length: usize) -> io::Result<u64> {
    let length = u64::try_from(length).map_err(|_| invalid("range length is too large"))?;
    offset.checked_add(length).ok_or_else(|| invalid("storage read range overflows"))
}

fn short_range() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "storage read range exceeds the file length")
}

/// Accumulate only bytes actually read; hostile offsets/lengths never preallocate a huge buffer.
fn read_bytes(mut reader: impl io::Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        bytes.try_reserve(n).map_err(io::Error::other)?;
        bytes.extend_from_slice(chunk.get(..n).ok_or_else(|| io::Error::other("invalid storage reader length"))?);
    }
}

fn copy_bytes(source: &[u8]) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(source.len()).map_err(io::Error::other)?;
    bytes.extend_from_slice(source);
    Ok(bytes)
}

fn sync_directory(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    } // Windows directory handles are not available through std.
}

/// In-memory files. Clones share the same files (so a test can "reopen" or corrupt them).
#[derive(Clone)]
pub struct MemStore {
    namespace: Option<u64>,
    pub files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    /// Offer a [`Store::background_writer`] (off by default: snapshots stay synchronous).
    pub background: bool,
}

impl Default for MemStore {
    fn default() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // Never wrap/reuse identity while a retired debt registry may outlive
        // the backing allocation. Exhaustion returns unknown/protected identity;
        // clones carry the same namespace and files.
        let namespace = NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1)).ok();
        Self { namespace, files: Arc::default(), background: false }
    }
}

impl MemStore {
    pub fn new() -> MemStore {
        MemStore::default()
    }
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.files.lock().unwrap_or_else(|e| e.into_inner()).get(name).cloned()
    }
    pub fn set(&self, name: &str, data: Vec<u8>) {
        self.files.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), data);
    }
}

impl Store for MemStore {
    fn migration_writer(&self) -> Option<Box<dyn Store>> {
        Some(Box::new(self.clone()))
    }
    fn source_snapshot(&mut self, name: &str) -> io::Result<Option<Box<dyn SourceSnapshot>>> {
        validate_name(name)?;
        source::memory_snapshot(self, name)
    }
    fn copy_source_atomic(&mut self, name: &str, source: &mut dyn SourceSnapshot) -> io::Result<()> {
        validate_name(name)?;
        let mut bytes = Vec::new();
        source::copy(source, &mut bytes)?;
        self.set(name, bytes);
        Ok(())
    }
    fn replace_source_atomic(&mut self, name: &str, expected: &SourceIdentity, data: &[u8]) -> io::Result<()> {
        self.replace_source_atomic_authorized(name, expected, data, &|| true)
    }
    fn replace_source_atomic_authorized(
        &mut self,
        name: &str,
        expected: &SourceIdentity,
        data: &[u8],
        authorized: &dyn Fn() -> bool,
    ) -> io::Result<()> {
        source::authority(authorized)?;
        validate_name(name)?;
        let mut files = self.files.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bytes = files.get(name).ok_or_else(source::changed)?;
        if source::memory_identity(self, bytes) != *expected {
            return Err(source::changed());
        }
        let replacement = copy_bytes(data)?;
        source::authority(authorized)?;
        files.insert(name.to_owned(), replacement);
        Ok(())
    }
    fn read(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        validate_name(name)?;
        Ok(self.get(name))
    }
    fn read_limited(&mut self, name: &str, max_bytes: u64) -> io::Result<Option<Vec<u8>>> {
        validate_name(name)?;
        max_bytes.checked_add(1).ok_or_else(|| invalid("bounded read limit is too large"))?;
        let files = self.files.lock().unwrap_or_else(|e| e.into_inner());
        let Some(bytes) = files.get(name) else { return Ok(None) };
        if bytes.len() as u64 > max_bytes {
            return Err(oversized(max_bytes));
        }
        Ok(Some(copy_bytes(bytes)?))
    }
    fn read_range(&mut self, name: &str, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        validate_name(name)?;
        let end = range_end(offset, length)?;
        let files = self.files.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = files.get(name).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "storage file is missing"))?;
        if end > bytes.len() as u64 {
            return Err(short_range());
        }
        let start = usize::try_from(offset).map_err(|_| short_range())?;
        let end = usize::try_from(end).map_err(|_| short_range())?;
        copy_bytes(bytes.get(start..end).ok_or_else(short_range)?)
    }
    fn remove(&mut self, name: &str) -> io::Result<()> {
        validate_name(name)?;
        self.files.lock().unwrap_or_else(|e| e.into_inner()).remove(name);
        Ok(())
    }
    fn write_atomic(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        validate_name(name)?;
        self.set(name, data.to_vec());
        Ok(())
    }
    fn append(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        validate_name(name)?;
        self.files.lock().unwrap_or_else(|e| e.into_inner()).entry(name.to_string()).or_default().extend_from_slice(data);
        Ok(())
    }
    fn truncate(&mut self, name: &str, len: u64) -> io::Result<()> {
        validate_name(name)?;
        if let Some(f) = self.files.lock().unwrap_or_else(|e| e.into_inner()).get_mut(name) {
            f.truncate(usize::try_from(len).unwrap_or(usize::MAX));
        }
        Ok(())
    }
    fn describe(&self) -> String {
        "memory".into()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.namespace.map(|id| format!("memory-store:{id}"))
    }
    fn background_writer(&self) -> Option<Box<dyn Store>> {
        self.background.then(|| Box::new(self.clone()) as Box<dyn Store>)
    }
}

#[cfg(test)]
#[path = "tests_store.rs"]
mod tests;
