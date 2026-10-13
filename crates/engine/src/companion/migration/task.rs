//! Native source preparation owns its Store and existing LibraryLock until the
//! actual producer retires. Completion is data, never permission to activate a
//! root in a different Session/library. Small fallback continuations are finite.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
enum Message {
    Validated(Migration),
    Done(Result<Option<Migration>>),
    Activated(Migration, Result<bool>),
}
pub(crate) struct Task {
    core: Option<Migration>,
    reading: Option<Migration>,
    large: bool,
    recovery: bool,
    receiver: Option<mpsc::Receiver<Message>>,
    ready: Option<Option<Migration>>,
    alive: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    failure: Option<String>,
    activated: bool,
    generation: Option<u64>,
}
impl Drop for Task {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
struct Alive(Arc<AtomicBool>);
impl Drop for Alive {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
struct CheckedSource {
    source: Box<dyn SourceSnapshot>,
    cancel: Arc<AtomicBool>,
}
impl SourceSnapshot for CheckedSource {
    fn identity(&self) -> &SourceIdentity {
        self.source.identity()
    }
    fn read_range(&mut self, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        self.check()?;
        self.source.read_range(offset, length)
    }
    fn verify_current(&mut self) -> std::io::Result<()> {
        self.check()?;
        self.source.verify_current()
    }
    fn clone_reader(&self) -> std::io::Result<Box<dyn SourceSnapshot>> {
        self.check()?;
        self.source.clone_reader()
    }
}
impl CheckedSource {
    fn check(&self) -> std::io::Result<()> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "migration preparation cancelled; original source retained"));
        }
        Ok(())
    }
}
struct CheckedStore {
    store: Box<dyn Store>,
    cancel: Arc<AtomicBool>,
}
impl CheckedStore {
    fn check(&self) -> std::io::Result<()> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "migration preparation cancelled; original source retained"));
        }
        Ok(())
    }
}
impl Store for CheckedStore {
    fn read(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.check()?;
        self.store.read(name)
    }
    fn read_limited(&mut self, name: &str, max: u64) -> std::io::Result<Option<Vec<u8>>> {
        self.check()?;
        self.store.read_limited(name, max)
    }
    fn read_range(&mut self, name: &str, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        self.check()?;
        self.store.read_range(name, offset, length)
    }
    fn write_atomic(&mut self, name: &str, data: &[u8]) -> std::io::Result<()> {
        self.check()?;
        self.store.write_atomic(name, data)
    }
    fn remove(&mut self, name: &str) -> std::io::Result<()> {
        self.check()?;
        self.store.remove(name)
    }
    fn append(&mut self, name: &str, data: &[u8]) -> std::io::Result<()> {
        self.check()?;
        self.store.append(name, data)
    }
    fn truncate(&mut self, name: &str, len: u64) -> std::io::Result<()> {
        self.check()?;
        self.store.truncate(name, len)
    }
    fn describe(&self) -> String {
        self.store.describe()
    }
    fn retention_namespace(&self) -> Option<String> {
        self.store.retention_namespace()
    }
    fn conservative_retention(&self) -> bool {
        self.store.conservative_retention()
    }
    fn source_snapshot(&mut self, name: &str) -> std::io::Result<Option<Box<dyn SourceSnapshot>>> {
        self.check()?;
        self.store.source_snapshot(name)
    }
    fn copy_source_atomic(&mut self, name: &str, source: &mut dyn SourceSnapshot) -> std::io::Result<()> {
        self.check()?;
        self.store.copy_source_atomic(name, source)
    }
    fn replace_source_atomic(&mut self, _name: &str, _expected: &SourceIdentity, _data: &[u8]) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "migration preparation cannot activate the source"))
    }
    fn replace_source_atomic_authorized(
        &mut self,
        name: &str,
        expected: &SourceIdentity,
        data: &[u8],
        authorized: &dyn Fn() -> bool,
    ) -> std::io::Result<()> {
        self.check()?;
        let cancel = self.cancel.clone();
        self.store.replace_source_atomic_authorized(name, expected, data, &|| !cancel.load(Ordering::Acquire) && authorized())
    }
}
impl Task {
    pub fn open(store: &mut dyn Store) -> Result<Option<(bool, Option<Self>)>> {
        let large = match store.read_limited(STATE_FILE, 256 * 1024) {
            Ok(None) => return Ok(None),
            Ok(Some(_)) => false,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => true,
            Err(e) => return Err(error(e)),
        };
        let mut task = Self {
            core: None,
            reading: None,
            large,
            recovery: false,
            receiver: None,
            ready: None,
            alive: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            failure: None,
            activated: false,
            generation: None,
        };
        if !large {
            let source = match store.source_snapshot(STATE_FILE) {
                Ok(source) => source,
                Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
                    // Only unsupported adapters may read the established small
                    // v2 root, through the same strict duplicate-aware parser.
                    let Some(bytes) = store.read_limited(STATE_FILE, 32 * 1024).map_err(error)? else { return Ok(None) };
                    let memory = lightcraft_catalog::MemStore::new();
                    memory.set(STATE_FILE, bytes);
                    let mut memory = memory;
                    let source = memory.source_snapshot(STATE_FILE).map_err(error)?.ok_or_else(|| error("archive root missing"))?;
                    let (v2, _) = Migration::from_source(source)?;
                    return if v2 { Ok(Some((true, None))) } else { Err(error("legacy source streaming unsupported; original source retained")) };
                }
                Err(e) => return Err(error(e)),
            };
            let source = source.ok_or_else(|| error("archive source disappeared"))?;
            let (v2, core) = Migration::from_source(source)?;
            if v2 {
                if !Migration::needs_recovery(store)? {
                    return Ok(Some((true, None)));
                }
                task.recovery = true;
                if store.migration_writer().is_some() {
                    task.large = true;
                } else {
                    store.read_limited(BACKUP, 256 * 1024).map_err(error)?.ok_or_else(|| error("unconfirmed activation backup missing"))?;
                    task.core = Some(Migration::recover(store)?);
                }
                return Ok(Some((false, Some(task))));
            }
            if store.migration_writer().is_some() {
                // Every actual native/memory migration writer belongs to an
                // owned producer, including small sources on slow filesystems.
                task.reading = core;
                task.large = true;
            } else {
                task.core = core;
            }
        }
        Ok(Some((false, Some(task))))
    }
    pub fn busy(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
    pub fn retirement(&self) -> MigrationRetirement {
        MigrationRetirement { alive: self.alive.clone() }
    }
    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn retry(&mut self) {
        if !self.busy() {
            // Retirement implies the terminal message is already queued. Consume it
            // before clearing failure, and give the next producer its own receipt.
            let _ = self.poll();
            self.failure = None;
            self.cancel = Arc::new(AtomicBool::new(false));
            self.alive = Arc::new(AtomicBool::new(false));
        }
    }
    fn poll(&mut self) -> Result<()> {
        if let Some(receiver) = &self.receiver {
            while let Ok(message) = receiver.try_recv() {
                match message {
                    Message::Validated(reading) => {
                        self.reading = Some(reading);
                    }
                    Message::Done(result) => match result {
                        Ok(core) => self.ready = Some(core),
                        Err(e) => self.failure = Some(e.to_string()),
                    },
                    Message::Activated(core, result) => {
                        self.core = Some(core);
                        match result {
                            Ok(done) => self.activated = done,
                            Err(e) => self.failure = Some(e.to_string()),
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub fn drive(&mut self, store: &mut dyn Store, lock: Option<Arc<lightcraft_catalog::LibraryLock>>, generation: u64) -> Result<bool> {
        if self.generation.is_some_and(|owner| owner != generation) {
            self.cancel();
            return Err(error("migration completion belongs to a retired library"));
        }
        self.generation = Some(generation);
        self.poll()?;
        if self.cancel.load(Ordering::Acquire) {
            return Err(error("migration preparation cancelled; original source retained"));
        }
        if let Some(failure) = &self.failure {
            return Err(error(failure));
        }
        if self.busy() {
            return Ok(false);
        }
        if self.activated {
            return Ok(true);
        }
        if let Some(ready) = self.ready.take() {
            self.receiver = None;
            self.core = ready;
            if self.core.is_none() {
                return Ok(true);
            }
        }
        if self.core.is_some() {
            if !self.large {
                return self.core.as_mut().ok_or_else(|| error("migration continuation missing"))?.step(store);
            }
            let core = self.core.take().ok_or_else(|| error("prepared migration source missing"))?;
            let writer = store.migration_writer().ok_or_else(|| error("source publication requires an owned worker Store"))?;
            let (sender, receiver) = mpsc::sync_channel(1);
            self.receiver = Some(receiver);
            let cancel = self.cancel.clone();
            let alive = self.alive.clone();
            alive.store(true, Ordering::Release);
            let spawned = std::thread::Builder::new().name("companion-legacy-publication".into()).spawn(move || {
                let _alive = Alive(alive);
                let retained_lock = lock;
                let mut writer = CheckedStore { store: writer, cancel: cancel.clone() };
                let mut core = core;
                // Owning Session granted this revocable authority only after
                // preparation retired. The adapter checks it after the fresh
                // expensive source proof, immediately before atomic publication.
                let result = core.activate_prepared(&mut writer, &|| !cancel.load(Ordering::Acquire));
                drop(writer);
                let _ = sender.try_send(Message::Activated(core, result));
                drop(retained_lock);
            });
            if let Err(e) = spawned {
                self.alive.store(false, Ordering::Release);
                self.failure = Some(e.to_string());
                return Err(error(e));
            }
            return Ok(false);
        }
        if self.receiver.is_some() {
            self.receiver = None;
        }
        let writer =
            store.migration_writer().ok_or_else(|| error("large source migration requires an owned worker Store; original source retained"))?;
        let recovery = self.recovery;
        let source = self.reading.as_ref().map(|m| m.source.clone_reader().map_err(error)).transpose()?;
        let (sender, receiver) = mpsc::sync_channel(2);
        self.receiver = Some(receiver);
        let alive = self.alive.clone();
        let cancel = self.cancel.clone();
        alive.store(true, Ordering::Release);
        let spawned = std::thread::Builder::new().name("companion-legacy-migration".into()).spawn(move || {
            let _alive = Alive(alive);
            let retained_lock = lock;
            let mut writer = CheckedStore { store: writer, cancel: cancel.clone() };
            let result = (|| -> Result<Option<Migration>> {
                // Native open, initial full hash and structural scan all belong
                // to this retained producer, never to the owning GUI frame.
                let mut core = if recovery {
                    Migration::recover(&mut writer)?
                } else {
                    let source = match source {
                        Some(source) => source,
                        None => writer.source_snapshot(STATE_FILE).map_err(error)?.ok_or_else(|| error("legacy source disappeared"))?,
                    };
                    let source = Box::new(CheckedSource { source, cancel: cancel.clone() });
                    let (v2, core) = Migration::from_source(source)?;
                    if v2 {
                        return Ok(None);
                    }
                    core.ok_or_else(|| error("legacy source classification missing"))?
                };
                let reading = Migration { source: core.source.clone_reader().map_err(error)?, photos: core.photos, original: core.original.clone() };
                let _ = sender.try_send(Message::Validated(reading));
                while !core.prepared(&mut writer)? {
                    core.step(&mut writer)?;
                }
                core.verify_backup(&mut writer)?;
                core.source.verify_current().map_err(error)?;
                Ok(Some(core))
            })();
            // Store/OS-lock producer locals retire before the alive flag. A
            // queued completion's source reader stays owned by the Task.
            drop(writer);
            let _ = sender.try_send(Message::Done(result));
            drop(retained_lock);
        });
        if let Err(e) = spawned {
            self.alive.store(false, Ordering::Release);
            self.failure = Some(e.to_string());
            return Err(error(e));
        }
        Ok(false)
    }
    pub fn legacy_header(&mut self, store: &mut dyn Store, photo: PhotoId) -> Result<Option<super::super::storage::PhotoHeader>> {
        self.poll()?;
        if let Some(reading) = self.reading.as_mut() {
            return reading.legacy_header(store, photo);
        }
        if let Some(core) = self.core.as_mut() {
            return core.legacy_header(store, photo);
        }
        Ok(None)
    }
    pub fn conversation(&mut self, photo: PhotoId) -> Result<Option<Value>> {
        self.poll()?;
        if let Some(reading) = self.reading.as_mut() {
            return reading.conversation(photo);
        }
        if let Some(core) = self.core.as_mut() {
            return core.conversation(photo);
        }
        Err(error("legacy archive is being checked; original history is retained"))
    }
}
