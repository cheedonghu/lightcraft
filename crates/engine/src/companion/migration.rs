//! Verified v1 conversion. The original source remains authoritative until the
//! exact backup and every graph have durable proofs. Progress owns one bounded
//! linked completion record per photo, never an in-memory library-wide list.
use super::archive::{STATE_FILE, error};
use crate::{Result, Session};
use lightcraft_catalog::{PhotoId, SourceIdentity, SourceSnapshot, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
mod json;
mod task;
pub(crate) use json::{Json, Span};
pub(super) use task::Task;
/// Cancellation receipt retained by real shutdown/library-switch callers until
/// the actual owned Store/LibraryLock producer has retired. Dropping it does not
/// grant publication or cancel any other library.
#[derive(Clone)]
pub struct MigrationRetirement {
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl MigrationRetirement {
    pub fn retired(&self) -> bool {
        !self.alive.load(std::sync::atomic::Ordering::Acquire)
    }
}
const BACKUP: &str = "companion/state.v1.backup.json";
const SMALL: u64 = 32 * 1024;
const PROGRESS: &str = "companion/migration/progress.json";
#[derive(Serialize, Deserialize)]
struct Progress {
    source: SourceIdentity,
    photos: Span,
    cursor: u64,
    count: u64,
    completed: Option<String>,
    photos_complete: bool,
    verify: Option<String>,
    verified_count: u64,
    graphs_verified: bool,
    backup_verified: bool,
    #[serde(default)]
    activation_confirmed: bool,
}
#[derive(Serialize, Deserialize)]
struct Completed {
    photo: PhotoId,
    span: Span,
    header: super::storage::PhotoHeader,
    previous: Option<String>,
}
fn write<T: Serialize>(store: &mut dyn Store, key: &str, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(error)?;
    if bytes.len() as u64 > SMALL {
        return Err(error("migration progress exceeds bounded page size"));
    }
    store.write_atomic(key, &bytes).map_err(error)
}
fn read<T: serde::de::DeserializeOwned>(store: &mut dyn Store, key: &str) -> Result<Option<T>> {
    store.read_limited(key, SMALL).map_err(error)?.map(|b| serde_json::from_slice(&b).map_err(error)).transpose()
}
pub(super) struct Migration {
    source: Box<dyn SourceSnapshot>,
    photos: Span,
    original: Option<SourceIdentity>,
}
impl Migration {
    fn from_source(mut source: Box<dyn SourceSnapshot>) -> Result<(bool, Option<Self>)> {
        let root = json::root(source.as_mut())?;
        source.verify_current().map_err(error)?;
        if root.v2 {
            return Ok((true, None));
        }
        let photos = root.photos.ok_or_else(|| error("legacy photos missing"))?;
        Ok((false, Some(Self { source, photos, original: None })))
    }
    fn progress(&self, store: &mut dyn Store) -> Result<Progress> {
        if let Some(progress) = read::<Progress>(store, PROGRESS)? {
            if progress.source != *self.identity() || progress.photos != self.photos {
                return Err(error("migration source identity changed; preserve source, backup and staged graphs for recovery"));
            }
            if progress.cursor < self.photos.start || progress.cursor > self.photos.end || progress.verified_count > progress.count {
                return Err(error("invalid migration progress; source protected"));
            }
            return Ok(progress);
        }
        Ok(Progress {
            source: self.identity().clone(),
            photos: self.photos,
            cursor: self.photos.start.checked_add(1).ok_or_else(|| error("source cursor exhausted"))?,
            count: 0,
            completed: None,
            photos_complete: false,
            verify: None,
            verified_count: 0,
            graphs_verified: false,
            backup_verified: false,
            activation_confirmed: false,
        })
    }
    fn activated_root(&self) -> Result<Vec<u8>> {
        Self::root_for(self.identity())
    }
    fn identity(&self) -> &SourceIdentity {
        self.original.as_ref().unwrap_or_else(|| self.source.identity())
    }
    fn root_for(source: &SourceIdentity) -> Result<Vec<u8>> {
        serde_json::to_vec(&json!({"format":"lightcraft-companion","version":2,"layout":"photo-head-v2","legacy_backup":{"path":BACKUP,"length":source.length,"digest":source.digest}})).map_err(error)
    }
    fn needs_recovery(store: &mut dyn Store) -> Result<bool> {
        let Some(progress) = read::<Progress>(store, PROGRESS)? else { return Ok(false) };
        if progress.activation_confirmed {
            return Ok(false);
        }
        let current = store.read_limited(STATE_FILE, SMALL).map_err(error)?.ok_or_else(|| error("unconfirmed activation root missing"))?;
        if current != Self::root_for(&progress.source)? {
            return Err(error("unconfirmed migration root changed; source protected"));
        }
        Ok(true)
    }
    fn recover(store: &mut dyn Store) -> Result<Self> {
        let progress: Progress = read(store, PROGRESS)?.ok_or_else(|| error("activation progress missing"))?;
        if !progress.photos_complete || !progress.graphs_verified || !progress.backup_verified {
            return Err(error("unconfirmed activation lacks durable preparation proofs"));
        }
        let source = store.source_snapshot(BACKUP).map_err(error)?.ok_or_else(|| error("unconfirmed activation backup missing"))?;
        if source.identity().length != progress.source.length || source.identity().digest != progress.source.digest {
            return Err(error("unconfirmed activation backup identity mismatch"));
        }
        let (v2, core) = Self::from_source(source)?;
        if v2 {
            return Err(error("activation backup must retain original v1 source"));
        }
        let mut core = core.ok_or_else(|| error("activation backup missing legacy source"))?;
        if core.photos != progress.photos {
            return Err(error("activation backup photo span mismatch"));
        }
        core.original = Some(progress.source);
        Ok(core)
    }
    fn confirm_activation(&self, store: &mut dyn Store) -> Result<()> {
        let mut progress = self.progress(store)?;
        progress.activation_confirmed = true;
        write(store, PROGRESS, &progress)
    }
    fn verify_graphs(&self, store: &mut dyn Store) -> Result<()> {
        let progress = self.progress(store)?;
        let mut key = progress.completed;
        let mut count = 0u64;
        let mut later = self.photos.end;
        while let Some(current) = key {
            let done: Completed = read(store, &current)?.ok_or_else(|| error("activation completion proof missing"))?;
            if current != format!("companion/migration/{}/photo-{}.json", progress.source.digest, done.photo.0)
                || done.header.photo_id != done.photo
                || done.span.start < self.photos.start
                || done.span.end > later
                || done.span.start >= done.span.end
            {
                return Err(error("activation completion identity/order mismatch"));
            }
            later = done.span.start;
            super::storage::migration::verify(store, &done.header)?;
            key = done.previous;
            count = count.checked_add(1).ok_or_else(|| error("activation verification overflow"))?;
            if count > progress.count {
                return Err(error("activation completion cycle"));
            }
        }
        if count != progress.count {
            return Err(error("activation completion chain incomplete"));
        }
        Ok(())
    }
    /// One photo/proof/backup/publication continuation. CPU scans and streaming
    /// copies use fixed buffers, independently of source/photo/record size.
    pub fn step(&mut self, store: &mut dyn Store) -> Result<bool> {
        let activated = self.activated_root()?;
        if store.read_limited(STATE_FILE, SMALL).ok().flatten().as_deref() == Some(activated.as_slice()) {
            let progress = self.progress(store)?;
            if !progress.graphs_verified || !progress.backup_verified {
                return Err(error("activation has no durable migration proofs"));
            }
            self.verify_graphs(store)?;
            self.verify_backup(store)?;
            // Visibility is not durability. Repeat the identical small root
            // write after an ambiguous rename/directory-sync failure.
            let current = store.source_snapshot(STATE_FILE).map_err(error)?.ok_or_else(|| error("activation root missing"))?;
            store.replace_source_atomic(STATE_FILE, current.identity(), &activated).map_err(error)?;
            self.confirm_activation(store)?;
            return Ok(true);
        }
        self.source.read_range(0, 0).map_err(error)?;
        let mut progress = self.progress(store)?;
        write(store, PROGRESS, &progress)?;
        if !progress.photos_complete {
            let mut scanner = Json::new(self.source.as_mut(), progress.cursor);
            let mut first = progress.count == 0;
            if let Some((_, name, span)) = scanner.member(&mut first)? {
                let name = name.ok_or_else(|| error("legacy photo key exceeds numeric identity bounds"))?;
                let photo = PhotoId(name.parse::<u64>().map_err(error)?);
                if photo.0.to_string() != name {
                    return Err(error("ambiguous legacy photo identity"));
                }
                let key = format!("companion/migration/{}/photo-{}.json", progress.source.digest, photo.0);
                if let Some(done) = read::<Completed>(store, &key)? {
                    if done.photo != photo || done.span != span || done.previous != progress.completed {
                        return Err(error("duplicate legacy photo identity; source protected"));
                    }
                    super::storage::migration::verify(store, &done.header)?;
                    write(store, &key, &done)?;
                } else {
                    let header = super::storage::migration::stage(store, photo, self.source.as_mut(), span)?;
                    let done = Completed { photo, span, header, previous: progress.completed.clone() };
                    write(store, &key, &done)?;
                }
                // Keep stable source-to-transaction identity even if pending
                // cleanup reports an error after its removal became visible.
                super::storage::finish_pending(store, photo)?;
                progress.cursor = span.end;
                progress.count = progress.count.checked_add(1).ok_or_else(|| error("migration photo count exhausted"))?;
                progress.completed = Some(key);
            } else {
                if scanner.at != self.photos.end {
                    return Err(error("legacy photo object boundary mismatch"));
                }
                progress.photos_complete = true;
                progress.verify = progress.completed.clone();
            }
            write(store, PROGRESS, &progress)?;
            return Ok(false);
        }
        if !progress.graphs_verified {
            if let Some(key) = progress.verify.clone() {
                let done: Completed = read(store, &key)?.ok_or_else(|| error("migration completion page missing"))?;
                if key != format!("companion/migration/{}/photo-{}.json", progress.source.digest, done.photo.0) {
                    return Err(error("invalid migration completion reference"));
                }
                super::storage::migration::verify(store, &done.header)?;
                progress.verify = done.previous;
                progress.verified_count = progress.verified_count.checked_add(1).ok_or_else(|| error("migration verification exhausted"))?;
                if progress.verified_count > progress.count {
                    return Err(error("migration completion cycle"));
                }
            } else {
                if progress.verified_count != progress.count {
                    return Err(error("migration completion chain incomplete"));
                }
                progress.graphs_verified = true;
            }
            write(store, PROGRESS, &progress)?;
            return Ok(false);
        }
        if !progress.backup_verified {
            if let Some(existing) = store.source_snapshot(BACKUP).map_err(error)?
                && (existing.identity().length != progress.source.length || existing.identity().digest != progress.source.digest)
            {
                return Err(error("existing v1 backup conflicts; preserve it and original source for recovery"));
            }
            // Retry the same exact backup, including a prior error after rename.
            store.copy_source_atomic(BACKUP, self.source.as_mut()).map_err(error)?;
            self.verify_backup(store)?;
            progress.backup_verified = true;
            write(store, PROGRESS, &progress)?;
            return Ok(false);
        }
        self.verify_graphs(store)?;
        self.verify_backup(store)?;
        self.source.verify_current().map_err(error)?;
        store.replace_source_atomic(STATE_FILE, &progress.source, &activated).map_err(error)?;
        self.confirm_activation(store)?;
        Ok(true)
    }
    fn verify_backup(&self, store: &mut dyn Store) -> Result<()> {
        let mut backup = store.source_snapshot(BACKUP).map_err(error)?.ok_or_else(|| error("exact v1 backup missing"))?;
        if backup.identity().length != self.identity().length || backup.identity().digest != self.identity().digest {
            return Err(error("exact v1 backup verification failed"));
        }
        backup.verify_current().map_err(error)
    }
    fn prepared(&self, store: &mut dyn Store) -> Result<bool> {
        let p = self.progress(store)?;
        Ok(p.photos_complete && p.graphs_verified && p.backup_verified)
    }
    fn activate_prepared(&mut self, store: &mut dyn Store, authorized: &dyn Fn() -> bool) -> Result<bool> {
        let activated = self.activated_root()?;
        if store.read_limited(STATE_FILE, SMALL).ok().flatten().as_deref() == Some(activated.as_slice()) {
            self.verify_graphs(store)?;
            self.verify_backup(store)?;
            let current = store.source_snapshot(STATE_FILE).map_err(error)?.ok_or_else(|| error("activation root missing"))?;
            store.replace_source_atomic_authorized(STATE_FILE, current.identity(), &activated, authorized).map_err(error)?;
            self.confirm_activation(store)?;
            return Ok(true);
        }
        if !self.prepared(store)? {
            return Err(error("migration preparation proof missing"));
        }
        self.verify_graphs(store)?;
        self.verify_backup(store)?;
        // The generic conditional writer revalidates the whole current source
        // digest immediately before publication; only this owning Session calls it.
        store.replace_source_atomic_authorized(STATE_FILE, self.identity(), &activated, authorized).map_err(error)?;
        self.confirm_activation(store)?;
        Ok(true)
    }
    fn legacy_header(&mut self, store: &mut dyn Store, photo: PhotoId) -> Result<Option<super::storage::PhotoHeader>> {
        // Source selected compatibility is read-only and never admits a v2
        // selector or mutation while the original root remains authoritative.
        let original_read = self.source.read_range(0, 0);
        if original_read.is_err() || self.original.is_some() {
            let current = store.read_limited(STATE_FILE, SMALL).map_err(error)?.ok_or_else(|| error("unconfirmed activation root missing"))?;
            let progress = self.progress(store)?;
            if current != self.activated_root()? || !progress.graphs_verified || !progress.backup_verified {
                return Err(error("legacy source/activation identity changed; original history protected"));
            }
        }
        let key = format!("companion/migration/{}/photo-{}.json", self.identity().digest, photo.0);
        let Some(done) = read::<Completed>(store, &key)? else { return Ok(None) };
        if done.photo != photo
            || done.header.photo_id != photo
            || !done.header.legacy_migrated
            || done.span.start < self.photos.start
            || done.span.end > self.photos.end
        {
            return Err(error("legacy compatibility completion identity mismatch"));
        }
        super::storage::migration::checked_header(store, &done.header).map(Some)
    }
    pub fn conversation(&mut self, photo: PhotoId) -> Result<Option<Value>> {
        let mut scanner = Json::new(self.source.as_mut(), self.photos.start);
        scanner.expect(b'{')?;
        let mut first = true;
        while let Some((_, name, span)) = scanner.member(&mut first)? {
            if name.as_deref() == Some(photo.0.to_string().as_str()) {
                if span.len()? > 32 * 1024 * 1024 {
                    return Err(error("large legacy conversation awaits bounded migration pages; all source bytes retained"));
                }
                let mut bytes = Vec::new();
                scanner.copy(span, &mut bytes)?;
                let mut value: Value = serde_json::from_slice(&bytes).map_err(error)?;
                if let Some(checkpoint) = value.get_mut("checkpoint").filter(|v| v.is_object()) {
                    checkpoint["eligible"] = json!(false);
                }
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
}
impl Session {
    pub fn companion_migration_pending(&self) -> bool {
        self.companion_archive.migration.is_some()
    }
    pub fn companion_migration_error(&self) -> Option<&str> {
        self.companion_archive.migration.as_ref().and(self.companion_archive.error.as_deref())
    }
    /// Explicit storage-only retry, shared with normal persistence. No edits or
    /// backend requests are replayed.
    pub fn retry_companion_migration(&mut self) -> Result<bool> {
        if let Some(task) = self.companion_archive.migration.as_mut() {
            task.retry();
        }
        // This queues storage work. Automatic persistence respects the owning
        // frontend's first-feedback defer and aggregate backoff; Save stays explicit.
        self.companion_archive.retry_now();
        Ok(!self.companion_migration_pending())
    }
    pub fn companion_archive_protected_reason(&self) -> Option<&str> {
        self.companion_archive.blocked.as_deref()
    }
    pub fn companion_legacy_photo_header(&mut self, photo: PhotoId) -> Result<Option<super::storage::PhotoHeader>> {
        let Some(task) = self.companion_archive.migration.as_mut() else { return Ok(None) };
        let store = self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut();
        task.legacy_header(store, photo)
    }
    pub fn companion_migration_busy(&self) -> bool {
        self.companion_archive.migration.as_ref().is_some_and(Task::busy)
    }
    pub fn cancel_companion_migration(&mut self) -> Option<MigrationRetirement> {
        self.companion_archive.migration.as_mut().map(|task| {
            task.cancel();
            task.retirement()
        })
    }
}

/// Compatibility v1 writes check the actual current root immediately before
/// publication; a stale in-memory Archive cannot overwrite a later activation.
pub(super) fn write_legacy(store: &mut dyn Store, bytes: &[u8]) -> Result<()> {
    let source = store.source_snapshot(STATE_FILE).map_err(error)?;
    if let Some(mut source) = source {
        let root = json::root(source.as_mut())?;
        if root.v2 {
            return Err(error("archive was upgraded; reopen before changing legacy history"));
        }
        store.replace_source_atomic(STATE_FILE, source.identity(), bytes).map_err(error)
    } else {
        store.write_atomic(STATE_FILE, bytes).map_err(error)
    }
}

pub(super) fn known_v2_root(bytes: &[u8]) -> Result<bool> {
    let mut memory = lightcraft_catalog::MemStore::new();
    memory.set(STATE_FILE, bytes.to_vec());
    let mut source = memory.source_snapshot(STATE_FILE).map_err(error)?.ok_or_else(|| error("archive root missing"))?;
    Ok(json::root(source.as_mut())?.v2)
}
