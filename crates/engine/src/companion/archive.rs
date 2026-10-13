//! Companion-owned persistence. It never adds catalog fields, ops or format versions.
use crate::{EngineError, Result, Session};
use lightcraft_catalog::{PhotoId, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const STATE_FILE: &str = "companion/state.json";
const MAX_ARCHIVE: u64 = 64 << 20;

#[derive(Serialize, Deserialize)]
struct Saved {
    format: String,
    version: u32,
    photos: BTreeMap<PhotoId, Value>,
}

#[derive(Default)]
pub(crate) struct Archive {
    pub(super) editors: super::editor::Editors,
    pub(super) photos: BTreeMap<PhotoId, Value>,
    pub(super) legacy: bool,
    pub(super) v2: bool,
    pub(super) tails: BTreeMap<PhotoId, super::storage::TailUpdate>,
    pub(super) prefixes: BTreeMap<PhotoId, super::storage::PhotoHeader>,
    pub(super) cleanup_photos: BTreeSet<PhotoId>,
    pub(super) cleanup_errors: BTreeMap<PhotoId, String>,
    pub(super) cleanup_published: BTreeMap<PhotoId, String>,
    cleanup_at: Option<web_time::Instant>,
    cleanup_after: Option<PhotoId>,
    pub(super) receipts: BTreeMap<PhotoId, Vec<Value>>,
    pub(super) readers: std::sync::Arc<super::storage::reader_pins::Pins>,
    pub(super) dirty_photos: BTreeSet<PhotoId>,
    pub(super) revert_dirty: BTreeSet<PhotoId>,
    dirty: bool,
    pub(super) blocked: Option<String>,
    pub(super) error: Option<String>,
    retry_at: Option<web_time::Instant>,
}
pub(super) fn error(e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("AI archive: {e}"))
}
impl Archive {
    pub(crate) fn load(store: &mut dyn Store, _dir: &Path, _on_disk: bool) -> Self {
        let pins = super::storage::reader_pins::for_store(store);
        let mut read = || -> Result<Self> {
            let Some(bytes) = store.read_limited(STATE_FILE, MAX_ARCHIVE).map_err(error)? else { return Ok(Self::default()) };
            let root: Value = serde_json::from_slice(&bytes).map_err(error)?;
            if root["format"] == "lightcraft-companion" && root["version"] == 2 && root["layout"] == "photo-head-v2" {
                return Ok(Self { v2: true, ..Default::default() });
            }
            let saved: Saved = serde_json::from_slice(&bytes).map_err(error)?;
            if saved.format != "lightcraft-companion" || saved.version != 1 {
                return Err(error("unsupported format; the file was left unchanged"));
            }
            Ok(Self { photos: saved.photos, legacy: true, ..Default::default() })
        };
        match read() {
            Ok(mut a) => {
                a.readers = pins;
                a
            }
            Err(e) => {
                log::error!("{e}");
                Self { blocked: Some(e.to_string()), readers: pins, ..Default::default() }
            }
        }
    }
    pub(crate) fn invalidate(&mut self, ids: &BTreeSet<PhotoId>) {
        for id in ids {
            if let Some(update) = self.tails.get_mut(id)
                && update.metadata.pointer("/checkpoint/eligible").and_then(Value::as_bool) == Some(true)
            {
                update.metadata["checkpoint"]["eligible"] = json!(false);
            }
            if let Some(a) = self.photos.get_mut(id)
                && a.pointer("/checkpoint/eligible").and_then(Value::as_bool) == Some(true)
            {
                a["checkpoint"]["eligible"] = json!(false);
                self.dirty = true;
                if self.v2 {
                    self.dirty_photos.insert(*id);
                }
            }
        }
    }
    pub(crate) fn invalidate_storage(&mut self, library: &mut crate::library::Library, ids: &BTreeSet<PhotoId>) {
        if !self.v2 {
            return;
        }
        self.revert_dirty.extend(ids.iter().copied());
        match super::storage::invalidate_revert(library.files.as_mut(), &self.revert_dirty) {
            Ok(()) => {
                self.revert_dirty.clear();
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }
    }
    pub(crate) fn needs_retry(&self) -> bool {
        let data = self.dirty
            || !self.tails.is_empty()
            || !self.dirty_photos.is_empty()
            || !self.revert_dirty.is_empty()
            || !self.receipts.is_empty()
            || self.editors.dirty();
        (data && self.retry_at.is_none_or(|t| web_time::Instant::now() >= t))
            || (!data && !self.cleanup_photos.is_empty() && self.cleanup_at.is_none_or(|t| web_time::Instant::now() >= t))
    }
    fn cleanup_step(&mut self, store: &mut dyn Store) {
        use std::ops::Bound::{Excluded, Unbounded};
        let photo = self
            .cleanup_after
            .and_then(|p| self.cleanup_photos.range((Excluded(p), Unbounded)).next().copied())
            .or_else(|| self.cleanup_photos.first().copied());
        let Some(photo) = photo else { return };
        self.cleanup_after = Some(photo);
        if !self.tails.contains_key(&photo)
            && !self.dirty_photos.contains(&photo)
            && !self.receipts.contains_key(&photo)
            && let Err(e) = super::storage::reconcile_visible_pending(store, photo, self.cleanup_published.get(&photo).map(String::as_str))
        {
            self.cleanup_errors.insert(photo, e.to_string());
            self.cleanup_at = web_time::Instant::now().checked_add(std::time::Duration::from_millis(250));
            return;
        }
        self.cleanup_published.remove(&photo);
        let mut retained: Vec<_> =
            self.prefixes.get(&photo).into_iter().cloned().chain(self.tails.get(&photo).and_then(|t| t.prefix.as_ref()).cloned()).collect();
        let readers = self.readers.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = if self.readers.conservative && !readers.headers.is_empty() {
            // Windows uses a shared native protection domain. Do not mix other
            // libraries' headers into this photo's graph or report them unsaved.
            Ok(super::storage::CleanupReport { pending: true, removed: 0 })
        } else {
            retained.extend(readers.headers.values().filter(|h| h.photo_id == photo).cloned());
            super::storage::collect(store, photo, retained, 2)
        };
        match result {
            Ok(report) => {
                if !report.pending {
                    self.cleanup_photos.remove(&photo);
                }
                self.cleanup_errors.remove(&photo);
            }
            Err(e) => {
                self.cleanup_errors.insert(photo, e.to_string());
            }
        }
        self.cleanup_at = web_time::Instant::now().checked_add(std::time::Duration::from_millis(250));
    }
    pub(crate) fn unsaved(&self) -> Option<&str> {
        self.error
            .as_deref()
            .filter(|_| {
                self.dirty || !self.tails.is_empty() || !self.dirty_photos.is_empty() || !self.revert_dirty.is_empty() || !self.receipts.is_empty()
            })
            .or_else(|| self.editors.unsaved())
    }
    fn persist(&mut self, library: &mut crate::library::Library) -> Result<()> {
        if let Some(e) = &self.blocked {
            if self.dirty || !self.tails.is_empty() || !self.dirty_photos.is_empty() || !self.revert_dirty.is_empty() || !self.receipts.is_empty() {
                return Err(error(e));
            }
            return Ok(());
        }
        if self.v2 || !self.tails.is_empty() || !self.dirty_photos.is_empty() {
            let mut tail_failed = None;
            for photo in self.receipts.keys().copied().collect::<Vec<_>>() {
                let queued = self.receipts.get(&photo).cloned().unwrap_or_default();
                let mut left = Vec::new();
                for receipt in queued {
                    if let Err(e) = super::storage::retry_receipt(library.files.as_mut(), photo, &receipt) {
                        tail_failed = Some(e.to_string());
                        left.push(receipt);
                    } else {
                        self.cleanup_photos.insert(photo);
                        if let Ok(Some(transaction)) = super::storage::published_pending_transaction(library.files.as_mut(), photo) {
                            self.cleanup_published.insert(photo, transaction);
                        }
                    }
                }
                if left.is_empty() {
                    self.receipts.remove(&photo);
                } else {
                    self.receipts.insert(photo, left);
                }
            }
            let mut finished = BTreeMap::new();
            for photo in self.tails.keys().copied().collect::<Vec<_>>() {
                let Some(update) = self.tails.get(&photo) else { continue };
                match super::storage::persist_tail(library.files.as_mut(), photo, update) {
                    Ok(transaction) => {
                        finished.insert(photo, transaction);
                    }
                    Err(error) => {
                        tail_failed = Some(error.to_string());
                    }
                }
            }
            let mut failed = tail_failed;
            for photo in self.revert_dirty.clone() {
                match super::storage::invalidate_revert(library.files.as_mut(), &BTreeSet::from([photo])) {
                    Ok(()) => {
                        self.revert_dirty.remove(&photo);
                    }
                    Err(e) => {
                        failed = Some(e.to_string());
                    }
                }
            }
            for photo in self.dirty_photos.clone() {
                if let Some(value) = self.photos.get(&photo) {
                    match super::storage::stored_epoch(library.files.as_mut(), photo)
                        .and_then(|epoch| super::storage::stage(library.files.as_mut(), photo, epoch, value))
                        .and_then(|p| super::storage::commit(library.files.as_mut(), &p).map(|()| p.transaction_id))
                    {
                        Ok(transaction) => {
                            finished.insert(photo, transaction);
                        }
                        Err(e) => {
                            failed = Some(e.to_string());
                        }
                    }
                }
            }
            if failed.is_none() && !self.v2 {
                match super::storage::activate(library.files.as_mut()) {
                    Ok(()) => {
                        self.v2 = true;
                    }
                    Err(e) => {
                        failed = Some(e.to_string());
                        self.dirty_photos.extend(self.photos.keys().copied());
                    }
                }
            }
            if self.v2 {
                for (photo, transaction) in finished {
                    self.tails.remove(&photo);
                    self.dirty_photos.remove(&photo);
                    self.photos.remove(&photo);
                    self.cleanup_photos.insert(photo);
                    self.cleanup_published.insert(photo, transaction);
                    if let Err(e) = super::storage::finish_pending(library.files.as_mut(), photo) {
                        self.cleanup_errors.insert(photo, e.to_string());
                    }
                }
            }
            self.dirty = !self.tails.is_empty() || !self.dirty_photos.is_empty();
            self.error = failed.clone();
            self.retry_at = failed.as_ref().map(|_| web_time::Instant::now() + std::time::Duration::from_secs(2));
            self.cleanup_step(library.files.as_mut());
            return failed.map_or(Ok(()), |e| Err(EngineError::NotSaved(e)));
        }
        if !self.dirty {
            return Ok(());
        }
        if let Some(e) = &self.blocked {
            return Err(error(e));
        }
        let result = (|| {
            let saved = Saved { format: "lightcraft-companion".into(), version: 1, photos: self.photos.clone() };
            let bytes = serde_json::to_vec(&saved).map_err(error)?;
            if bytes.len() as u64 > MAX_ARCHIVE {
                return Err(error("file exceeds 64 MiB"));
            }
            library.files.write_atomic(STATE_FILE, &bytes).map_err(error)
        })();
        match result {
            Ok(()) => {
                self.dirty = false;
                self.error = None;
                self.retry_at = None;
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.to_string());
                self.retry_at = Some(web_time::Instant::now() + std::time::Duration::from_secs(2));
                Err(EngineError::NotSaved(e.to_string()))
            }
        }
    }
}
impl Session {
    pub fn photo_companion(&self, photo: PhotoId) -> Result<Option<&Value>> {
        if let Some(e) = &self.companion_archive.blocked {
            return Err(error(e));
        }
        Ok(self.companion_archive.photos.get(&photo))
    }
    pub fn set_photo_companion(&mut self, photo: PhotoId, conversation: Option<Value>) -> Result<()> {
        if self.catalog.photo(photo).is_none() {
            return Err(error("no such photo"));
        }
        if let Some(e) = &self.companion_archive.blocked {
            return Err(error(e));
        }
        if self.companion_archive.v2 {
            return self.save_companion_conversation(photo, conversation.unwrap_or_else(|| json!({"schema_version":2,"messages":[],"rounds":[]})));
        }
        self.companion_archive.legacy = true;
        match conversation {
            Some(value) => {
                self.companion_archive.photos.insert(photo, value);
            }
            None => {
                self.companion_archive.photos.remove(&photo);
            }
        }
        self.companion_archive.dirty = true;
        self.flush_companion()
    }
    pub(crate) fn flush_companion(&mut self) -> Result<()> {
        match self.library.as_mut() {
            Some(lib) => {
                let editor_dirty = self.companion_archive.editors.dirty();
                if editor_dirty && !self.companion_archive.legacy && self.companion_archive.blocked.is_none() {
                    self.companion_archive.v2 = true;
                }
                let editors = self.companion_archive.editors.persist(
                    lib.files.as_mut(),
                    self.companion_archive.legacy,
                    self.companion_archive.blocked.as_deref(),
                );
                let conversation = self.companion_archive.persist(lib);
                let result = editors.and(conversation);
                // A successful conversation flush cannot cancel a failed draft's backoff.
                self.companion_archive.retry_at = result.as_ref().err().map(|_| web_time::Instant::now() + std::time::Duration::from_secs(2));
                result
            }
            None => Ok(()),
        }
    }
}

impl crate::library::Library {
    pub fn companion_attachment(&mut self, photo: PhotoId, data: &[u8]) -> Result<String> {
        if data.is_empty() || data.len() > 128 * 1024 {
            return Err(error("preview exceeds limits"));
        }
        let name = format!("companion/preview-{}-{:032x}.bin", photo.0, lightcraft_preview::hash_bytes(data).0);
        self.files.write_atomic(&name, data).map_err(error)?;
        Ok(name)
    }
    pub fn read_companion_attachment(&mut self, name: &str) -> Result<Vec<u8>> {
        if name.starts_with("companion/v2/") {
            return super::storage::read_preview(self.files.as_mut(), name);
        }
        validate_attachment(name)?;
        let bytes = self.files.read_limited(name, 128 * 1024).map_err(error)?.ok_or_else(|| error("preview is missing"))?;
        if bytes.is_empty() || bytes.len() > 128 * 1024 {
            return Err(error("preview exceeds limits"));
        }
        Ok(bytes)
    }
    pub fn remove_companion_attachment(&mut self, name: &str) -> Result<()> {
        validate_attachment(name)?;
        self.files.remove(name).map_err(error)
    }
}
fn validate_attachment(name: &str) -> Result<()> {
    let valid = name.len() <= 96
        && name.strip_prefix("companion/preview-").and_then(|s| s.strip_suffix(".bin")).is_some_and(|s| {
            s.split_once('-').is_some_and(|(id, hash)| {
                !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit())
            })
        });
    if !valid {
        return Err(error("invalid attachment reference"));
    }
    Ok(())
}

/// Predevelopment policy: discard only old AI ops, preserving record order and every photo op.
/// A known empty batch stands in for the removed op, retaining the upstream journal sequence.
pub(crate) fn discard_legacy_ops(store: &mut dyn Store) -> Result<()> {
    use lightcraft_catalog::journal::{LOG, SNAPSHOT, VERSION};
    let Some(log) = store.read(LOG).map_err(error)? else { return Ok(()) };
    // Old typed serializers emit this ASCII tag verbatim. Ordinary upstream journals need
    // no compatibility parsing or extra read of a potentially very large catalog snapshot.
    if !log.windows(b"setPhotoCompanion".len()).any(|w| w == b"setPhotoCompanion") {
        return Ok(());
    }
    if let Some(snap) = store.read(SNAPSHOT).map_err(error)? {
        #[derive(Deserialize)]
        struct Header {
            format: String,
            version: u32,
        }
        let value: Header = serde_json::from_slice(&snap).map_err(error)?;
        if value.format != "lightcraft-catalog" || value.version > VERSION {
            return Ok(());
        }
    }
    let text = match std::str::from_utf8(&log) {
        Ok(t) => t,
        Err(_) => return Ok(()),
    };
    let mut output = String::new();
    let mut changed = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let parsed = (|| {
            let (seq, rest) = trimmed.strip_prefix("{\"seq\":")?.split_once(",\"crc\":")?;
            let (crc, rest) = rest.split_once(",\"op\":")?;
            let body = rest.strip_suffix('}')?;
            let seq = seq.parse::<u64>().ok()?;
            if crc.parse::<u32>().ok()? != crc32fast::hash(body.as_bytes()) {
                return None;
            }
            Some((seq, body, serde_json::from_str::<Value>(body).ok()?))
        })();
        if let Some((seq, body, mut op)) = parsed {
            if remove_legacy(&mut op) {
                let known: lightcraft_catalog::Op = serde_json::from_value(op).map_err(error)?;
                output.push_str(&lightcraft_catalog::journal::encode_record(seq, &known));
                if line.ends_with('\n') {
                    output.push('\n');
                }
                changed = true;
                continue;
            }
            // A future CRC-valid op must be refused untouched by the upstream loader.
            if serde_json::from_str::<lightcraft_catalog::Op>(body).is_err() {
                return Ok(());
            }
        }
        output.push_str(line);
    }
    if changed {
        store.write_atomic(LOG, output.as_bytes()).map_err(error)?;
    }
    Ok(())
}
fn remove_legacy(op: &mut Value) -> bool {
    if op["op"] == "setPhotoCompanion" {
        *op = json!({"op":"batch","ops":[]});
        return true;
    }
    if op["op"] == "batch" {
        return op.get_mut("ops").and_then(Value::as_array_mut).is_some_and(|ops| {
            let mut changed = false;
            for op in ops {
                changed |= remove_legacy(op);
            }
            changed
        });
    }
    false
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod tests;
