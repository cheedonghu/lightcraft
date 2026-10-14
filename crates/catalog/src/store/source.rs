//! Identity-checked bounded source readers for format migration. Files are not
//! silently recopied into RAM. The native reader owns one file handle; the memory
//! reader shares immutable bytes and checks the captured revision under its mutex.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, Write};

const CHUNK: usize = 32 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {
    pub length: u64,
    pub digest: String,
    pub token: String,
}
pub trait SourceSnapshot: Send {
    fn identity(&self) -> &SourceIdentity;
    /// Exact range, at most 256 KiB, or an error if the captured source changed.
    fn read_range(&mut self, offset: u64, length: usize) -> io::Result<Vec<u8>>;
    fn verify_current(&mut self) -> io::Result<()>;
    /// Independent bounded reader of the same captured identity. No whole-file
    /// copy and no shared seek cursor.
    fn clone_reader(&self) -> io::Result<Box<dyn SourceSnapshot>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "source reader cloning is unsupported"))
    }
}
pub(super) fn changed() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "migration source changed; original source was left untouched")
}
fn bound(identity: &SourceIdentity, offset: u64, length: usize) -> io::Result<()> {
    if length > 256 * 1024 {
        return Err(invalid("snapshot ranges must be at most 256 KiB"));
    }
    if range_end(offset, length)? > identity.length {
        return Err(short_range());
    }
    Ok(())
}
fn stamp(metadata: &std::fs::Metadata) -> io::Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(format!(
            "{}:{}:{}:{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.len(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec()
        ))
    }
    #[cfg(not(unix))]
    {
        Ok(format!("{}:{:?}:{:?}", metadata.len(), metadata.modified()?, metadata.created().ok()))
    }
}
fn hash_reader(reader: &mut impl Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0; CHUNK];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(format!("{:x}", hasher.finalize()));
        }
        hasher.update(buffer.get(..count).ok_or_else(|| invalid("invalid source read"))?);
    }
}
struct Native {
    file: std::fs::File,
    path: PathBuf,
    identity: SourceIdentity,
}
pub(super) fn native_snapshot(path: PathBuf) -> io::Result<Option<Box<dyn SourceSnapshot>>> {
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(invalid("source is not a regular file"));
    }
    let token = stamp(&metadata)?;
    let digest = hash_reader(&mut file)?;
    if stamp(&file.metadata()?)? != token || stamp(&std::fs::metadata(&path)?)? != token {
        return Err(changed());
    }
    Ok(Some(Box::new(Native { file, path, identity: SourceIdentity { length: metadata.len(), digest, token } })))
}
pub(super) fn check_native(path: &Path, expected: &SourceIdentity) -> io::Result<()> {
    let captured = native_snapshot(path.to_owned())?.ok_or_else(changed)?;
    if captured.identity() != expected {
        return Err(changed());
    }
    Ok(())
}
impl Native {
    fn check_stamp(&self) -> io::Result<()> {
        if stamp(&self.file.metadata()?)? != self.identity.token || stamp(&std::fs::metadata(&self.path)?)? != self.identity.token {
            return Err(changed());
        }
        Ok(())
    }
}
impl SourceSnapshot for Native {
    fn identity(&self) -> &SourceIdentity {
        &self.identity
    }
    fn read_range(&mut self, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        bound(&self.identity, offset, length)?;
        self.check_stamp()?;
        self.file.seek(io::SeekFrom::Start(offset))?;
        let mut bytes = vec![0; length];
        self.file.read_exact(&mut bytes)?;
        self.check_stamp()?;
        Ok(bytes)
    }
    fn verify_current(&mut self) -> io::Result<()> {
        check_native(&self.path, &self.identity)
    }
    fn clone_reader(&self) -> io::Result<Box<dyn SourceSnapshot>> {
        self.check_stamp()?;
        let file = std::fs::File::open(&self.path)?;
        if stamp(&file.metadata()?)? != self.identity.token {
            return Err(changed());
        }
        Ok(Box::new(Native { file, path: self.path.clone(), identity: self.identity.clone() }))
    }
}
pub(super) fn memory_identity(store: &MemStore, file: &MemoryFile) -> io::Result<SourceIdentity> {
    let revision = file.revision.ok_or_else(|| invalid("memory source identity exhausted"))?;
    Ok(SourceIdentity {
        length: file.len() as u64,
        digest: format!("{:x}", Sha256::digest(&file.bytes)),
        token: format!("memory-source:{:?}:{revision}", store.namespace),
    })
}
struct Memory {
    store: MemStore,
    name: String,
    identity: SourceIdentity,
    file: Arc<MemoryFile>,
}
pub(super) fn memory_snapshot(store: &MemStore, name: &str) -> io::Result<Option<Box<dyn SourceSnapshot>>> {
    let files = store.files.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(bytes) = files.get(name) else { return Ok(None) };
    let identity = memory_identity(store, bytes)?;
    Ok(Some(Box::new(Memory { store: store.clone(), name: name.to_owned(), identity, file: bytes.clone() })))
}
impl SourceSnapshot for Memory {
    fn clone_reader(&self) -> io::Result<Box<dyn SourceSnapshot>> {
        Ok(Box::new(Memory { store: self.store.clone(), name: self.name.clone(), identity: self.identity.clone(), file: self.file.clone() }))
    }
    fn identity(&self) -> &SourceIdentity {
        &self.identity
    }
    fn read_range(&mut self, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        bound(&self.identity, offset, length)?;
        let files = self.store.files.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bytes = files.get(&self.name).ok_or_else(changed)?;
        if !Arc::ptr_eq(bytes, &self.file) {
            return Err(changed());
        }
        let start = usize::try_from(offset).map_err(|_| short_range())?;
        let end = start.checked_add(length).ok_or_else(short_range)?;
        copy_bytes(bytes.get(start..end).ok_or_else(short_range)?)
    }
    fn verify_current(&mut self) -> io::Result<()> {
        let files = self.store.files.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bytes = files.get(&self.name).ok_or_else(changed)?;
        if !Arc::ptr_eq(bytes, &self.file) || memory_identity(&self.store, bytes)? != self.identity {
            return Err(changed());
        }
        Ok(())
    }
}
pub(super) fn copy(source: &mut dyn SourceSnapshot, writer: &mut dyn Write) -> io::Result<()> {
    let mut cursor = 0;
    while cursor < source.identity().length {
        let length = (source.identity().length - cursor).min(CHUNK as u64) as usize;
        writer.write_all(&source.read_range(cursor, length)?)?;
        cursor = cursor.checked_add(length as u64).ok_or_else(|| invalid("source cursor overflow"))?;
    }
    source.verify_current()
}

pub(super) fn authority(authorized: &dyn Fn() -> bool) -> io::Result<()> {
    if authorized() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Interrupted, "source publication authority retired; original source retained"))
    }
}
