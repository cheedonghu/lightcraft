//! Durable round authority. The independent guard is never rebound to current
//! parameters or a newer conversation HEAD during load.
use super::mutation_guard::RuntimeRevert;
use super::{LocalRoundId, archive::error, storage};
use crate::{Result, Session};
use lightcraft_catalog::{PhotoId, Source, Store};
use lightcraft_develop::DevelopSettings;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

const LIMIT: u64 = 32 * 1024;
const IDENTITY: &str = "companion/v2/store-identity.json";
const CLOSE: &str = "companion/v2/close-proof.json";
const LOAD: &str = "companion/v2/load-proof.json";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Certificate {
    version: u32,
    photo: PhotoId,
    round: LocalRoundId,
    storage_identity: String,
    mutation_epoch: u64,
    conversation_epoch: u64,
    core_seq: u64,
    record: u64,
    original_digest: String,
    result_digest: String,
    source: Source,
    source_evidence: String,
    eligible: bool,
}
fn key(photo: PhotoId) -> String {
    format!("companion/v2/photos/{}/revert.json", photo.0)
}
pub(super) fn read_guard(store: &mut dyn Store, photo: PhotoId) -> Result<Option<Value>> {
    let Some(bytes) = store.read_limited(&key(photo), LIMIT).map_err(error)? else { return Ok(None) };
    let value: Value = serde_json::from_slice(&bytes).map_err(error)?;
    if !value.is_object() {
        return Err(error("unknown revert guard protected"));
    }
    if value.get("mutation_epoch").is_some_and(|v| v.as_u64().is_none()) || value.get("eligible").is_some_and(|v| v.as_bool().is_none()) {
        return Err(error("unknown revert guard protected"));
    }
    if let Some(cert) = value.get("certificate") {
        let cert: Certificate = serde_json::from_value(cert.clone()).map_err(error)?;
        if cert.version != 1 || cert.photo != photo {
            return Err(error("unknown revert certificate protected"));
        }
    }
    Ok(Some(value))
}
pub(super) fn write_guard(store: &mut dyn Store, photo: PhotoId, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(error)?;
    if bytes.len() as u64 > LIMIT {
        return Err(error("revert guard exceeds supported layout"));
    }
    store.write_atomic(&key(photo), &bytes).map_err(error)
}
fn seal_key(photo: PhotoId) -> String {
    format!("companion/v2/photos/{}/seal-pending.json", photo.0)
}
fn seal_pending(store: &mut dyn Store, photo: PhotoId) -> Result<bool> {
    let Some(bytes) = store.read_limited(&seal_key(photo), LIMIT).map_err(error)? else { return Ok(false) };
    let value: Value = serde_json::from_slice(&bytes).map_err(error)?;
    if value["version"] != 1 || value["photo"].as_u64() != Some(photo.0) || value["certificate_digest"].as_str().is_none() {
        return Err(error("unknown certificate sealing intent protected"));
    }
    Ok(true)
}
fn identity(store: &mut dyn Store, create: bool) -> Result<Option<String>> {
    if let Some(bytes) = store.read_limited(IDENTITY, LIMIT).map_err(error)? {
        let v: Value = serde_json::from_slice(&bytes).map_err(error)?;
        if v["version"] != 1 || v["identity"].as_str().is_none_or(str::is_empty) {
            return Err(error("unknown companion storage identity protected"));
        }
        return Ok(v["identity"].as_str().map(str::to_owned));
    }
    if !create {
        return Ok(None);
    }
    let id = LocalRoundId::new().map_err(error)?.as_str().to_owned();
    store.write_atomic(IDENTITY, &serde_json::to_vec(&json!({"version":1,"identity":id})).map_err(error)?).map_err(error)?;
    Ok(Some(id))
}
fn close_pending(store: &mut dyn Store) -> Result<Option<bool>> {
    let Some(bytes) = store.read_limited(CLOSE, LIMIT).map_err(error)? else { return Ok(None) };
    let value: Value = serde_json::from_slice(&bytes).map_err(error)?;
    if value["version"] != 1 || value["pending"].as_bool().is_none() {
        return Err(error("unknown companion close proof protected"));
    }
    Ok(value["pending"].as_bool())
}
fn digest(settings: &DevelopSettings) -> Result<String> {
    Ok(lightcraft_preview::hash_bytes(&serde_json::to_vec(settings).map_err(error)?).to_string())
}
fn load_pending(store: &mut dyn Store) -> Result<bool> {
    let Some(bytes) = store.read_limited(LOAD, LIMIT).map_err(error)? else { return Ok(false) };
    let value: Value = serde_json::from_slice(&bytes).map_err(error)?;
    if value["version"] != 1 || value["pending"] != true {
        return Err(error("unknown companion load proof protected"));
    }
    Ok(true)
}
/// Fence before core replay can repair or replace files and erase its warning.
pub(crate) fn begin_load(store: &mut dyn Store) -> Result<(bool, bool)> {
    let pending = load_pending(store)?;
    let failed_close = close_pending(store)? == Some(true);
    if pending || identity(store, false)?.is_some() {
        store.write_atomic(LOAD, br#"{"version":1,"pending":true}"#).map_err(error)?;
        return Ok((true, pending || failed_close));
    }
    Ok((false, false))
}
impl Session {
    /// Retire a photo's round authority before its conversation is cleared or
    /// repaired. The caller must first retire its task/lease and must not clear
    /// saved context if this returns an error. Photo edits/history stay intact.
    pub fn retire_companion_revert(&mut self, photo: PhotoId) -> Result<()> {
        self.preflight_mutation(&lightcraft_catalog::Op::RemovePhoto { id: photo })?;
        let result = self.invalidate_companion_reverts_before(&[photo].into_iter().collect());
        // Even a failed durable retirement makes existing permits stale and
        // prevents cold preparation from granting the old permission again.
        self.mutation_guard.reverts.remove(&photo);
        self.mutation_guard.loaded.insert(photo);
        result
    }
    /// Cheap local recovery feedback for the current photo; never source I/O.
    pub fn companion_revert_save_error(&self, photo: PhotoId) -> Option<&'static str> {
        if self.companion_archive.revert_dirty.contains(&photo) {
            Some("The revert protection record could not be saved. This edit was not applied; check storage and retry.")
        } else if self.companion_archive.certificate_dirty.contains(&photo) {
            Some("The AI result is kept, but its revert certificate was not saved. Retry saving.")
        } else {
            None
        }
    }
    pub(crate) fn finish_companion_load(&mut self) -> Result<()> {
        if !self.mutation_guard.load_fenced {
            return Ok(());
        }
        let Some(lib) = self.library.as_mut() else { return Ok(()) };
        if self.mutation_guard.retire_load {
            // Changing identity retires every old certificate, including photos
            // missing from a repaired/recreated catalog. Never refresh from equality.
            let id = LocalRoundId::new().map_err(error)?.as_str().to_owned();
            lib.files.write_atomic(IDENTITY, &serde_json::to_vec(&json!({"version":1,"identity":id})).map_err(error)?).map_err(error)?;
            self.mutation_guard.retire_load = false;
        }
        lib.files.remove(LOAD).map_err(error)?;
        self.mutation_guard.load_fenced = false;
        Ok(())
    }
    pub(crate) fn begin_companion_close(&mut self) -> Result<()> {
        let live = self.mutation_guard.reverts.values().any(|p| p.eligible);
        let Some(lib) = self.library.as_mut() else { return Ok(()) };
        if live || identity(lib.files.as_mut(), false)?.is_some() {
            // A durable fence rejects certificates until the final snapshot and
            // companion flush both finish. It also protects already sealed proofs.
            let _ = close_pending(lib.files.as_mut())?;
            lib.files.write_atomic(CLOSE, br#"{"version":1,"pending":true}"#).map_err(error)?;
        }
        Ok(())
    }
    pub(crate) fn finish_companion_close(&mut self) -> Result<()> {
        let Some(lib) = self.library.as_mut() else { return Ok(()) };
        if close_pending(lib.files.as_mut())?.is_some() {
            lib.files.remove(CLOSE).map_err(error)?;
        }
        Ok(())
    }
    pub(crate) fn invalidate_companion_reverts_before(&mut self, photos: &std::collections::BTreeSet<PhotoId>) -> Result<()> {
        let mut required = std::collections::BTreeSet::new();
        for photo in photos {
            let live = self.mutation_guard.reverts.get(photo).is_some_and(|p| p.eligible);
            if let Some(proof) = self.mutation_guard.reverts.get_mut(photo) {
                Arc::make_mut(proof).eligible = false;
            }
            // A later target may fail its guard read. Retiring this runtime
            // proof already creates debt, even before its first certificate.
            if live && self.library.is_some() {
                self.companion_archive.revert_dirty.insert(*photo);
            }
            self.companion_archive.certificate_dirty.remove(photo);
            let dirty = self.companion_archive.revert_dirty.contains(photo);
            let disk = match self.library.as_mut() {
                Some(lib) => read_guard(lib.files.as_mut(), *photo),
                None => Ok(None),
            };
            match disk {
                Ok(guard) => {
                    if live || dirty || guard.is_some_and(|g| g["eligible"] == true || g["certificate"]["eligible"] == true) {
                        required.insert(*photo);
                    }
                }
                Err(e) => {
                    self.companion_archive.revert_dirty.insert(*photo);
                    self.companion_archive.error = Some(e.to_string());
                    return Err(crate::EngineError::Other(
                        "The revert protection record could not be saved. This edit was not applied; check storage and retry.".into(),
                    ));
                }
            }
        }
        if let Some(lib) = self.library.as_mut() {
            self.companion_archive.revert_dirty.extend(required.iter().copied());
            if let Err(e) = storage::invalidate_revert(lib.files.as_mut(), &required) {
                self.companion_archive.error = Some(e.to_string());
                return Err(crate::EngineError::Other(
                    "The revert protection record could not be saved. This edit was not applied; check storage and retry.".into(),
                ));
            }
            for photo in required {
                self.companion_archive.revert_dirty.remove(&photo);
            }
        }
        Ok(())
    }
    pub(super) fn fresh_revert_source(&self, source: &Source) -> Result<String> {
        match source {
            Source::Demo { scene } => {
                let scene =
                    lightcraft_scenes::demo_library().into_iter().find(|s| s.id == *scene).ok_or_else(|| error("original source unavailable"))?;
                // Definition and uncached deterministic generator output, including
                // float radiance, detect generator changes across application versions.
                let mut h = lightcraft_preview::Hasher128::new();
                h.update(&serde_json::to_vec(&scene).map_err(error)?);
                for pixel in scene.render_fit_serial(32).data {
                    for value in pixel {
                        h.update(&value.to_le_bytes());
                    }
                }
                Ok(format!("demo:{}", h.finish()))
            }
            Source::File { path } => {
                // No decoded/smart preview, cached content_hash or path/size key
                // establishes original bytes. Native hosts must install source hooks.
                if self.media.file_probe.is_none() {
                    return Err(error("original source unavailable"));
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    use std::io::Read;
                    let mut file = std::fs::File::open(path).map_err(error)?;
                    let mut h = lightcraft_preview::Hasher128::new();
                    let mut buffer = [0u8; 64 * 1024];
                    loop {
                        let n = file.read(&mut buffer).map_err(error)?;
                        if n == 0 {
                            break;
                        }
                        h.update(buffer.get(..n).ok_or_else(|| error("source read exceeded buffer"))?);
                    }
                    Ok(format!("native:{}", h.finish()))
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = path;
                    Err(error("original source unavailable"))
                }
            }
        }
    }
    fn certificate_record(&mut self, photo: PhotoId, ordinal: u64) -> Result<Option<storage::RevertRecord>> {
        let Some(h) = self.companion_photo_header(photo)? else { return Ok(None) };
        if ordinal >= h.record_count {
            return Ok(None);
        }
        Ok(Some(storage::revert_record(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), &h, ordinal)?))
    }
    pub(super) fn capture_companion_revert_record(&mut self, photo: PhotoId, epoch: u64, start: u64, records: &[Value]) {
        let Some(proof) = self.mutation_guard.reverts.get_mut(&photo).filter(|p| p.eligible && p.record.is_none()) else { return };
        if let Some(index) = records.iter().position(|r| {
            r["local_round_id"] == proof.round.as_str()
                && r["original"] == json!(proof.original)
                && r["recent_complete_result"] == json!(proof.expected)
        }) && let Some(ordinal) = start.checked_add(index as u64)
        {
            // This locates an already trusted publication after its data commit;
            // it never grants authority, changes its frozen effect, or runs on load.
            Arc::make_mut(proof).record = Some((epoch, ordinal));
        }
    }
    /// Qualify one cold photo once, before displaying its history. Failed proof
    /// verification leaves history readable and cannot create a runtime candidate.
    pub fn prepare_companion_revert(&mut self, photo: PhotoId) -> Result<()> {
        if self.mutation_guard.reverts.contains_key(&photo) || self.mutation_guard.loaded.contains(&photo) {
            return Ok(());
        }
        self.mutation_guard.loaded.insert(photo);
        if !self.companion_archive.v2 || self.companion_archive.blocked.is_some() {
            return Ok(());
        }
        let Some(lib) = self.library.as_mut() else { return Ok(()) };
        let Some(guard) = read_guard(lib.files.as_mut(), photo)? else { return Ok(()) };
        let Some(value) = guard.get("certificate") else { return Ok(()) };
        let cert: Certificate = serde_json::from_value(value.clone()).map_err(error)?;
        if cert.version != 1
            || cert.photo != photo
            || !cert.eligible
            || guard["mutation_epoch"].as_u64() != Some(cert.mutation_epoch)
            || !self.mutation_guard.clean_load
            || self.mutation_guard.loaded_seq != cert.core_seq
            || seal_pending(lib.files.as_mut(), photo)?
            || close_pending(lib.files.as_mut())? == Some(true)
            || load_pending(lib.files.as_mut())?
            || identity(lib.files.as_mut(), false)?.as_deref() != Some(cert.storage_identity.as_str())
        {
            return Ok(());
        }
        let Some(h) = self.companion_photo_header(photo)? else { return Ok(()) };
        if h.conversation_epoch != cert.conversation_epoch || self.companion_history_unsaved(photo) {
            return Ok(());
        }
        let Some(record) = self.certificate_record(photo, cert.record)? else { return Ok(()) };
        if record.local_round_id != cert.round.as_str() {
            return Ok(());
        }
        let original = record.original;
        let expected = record.recent_complete_result;
        if digest(&original)? != cert.original_digest || digest(&expected)? != cert.result_digest {
            return Ok(());
        }
        if self.fresh_revert_source(&cert.source).ok().as_deref() != Some(cert.source_evidence.as_str()) {
            self.invalidate_companion_reverts_before(&[photo].into_iter().collect())?;
            return Ok(());
        }
        let Some(p) = self.catalog.photo(photo) else { return Ok(()) };
        if p.deleted || p.source != cert.source || *p.develop != expected {
            return Ok(());
        }
        self.mutation_guard.reverts.insert(
            photo,
            Arc::new(RuntimeRevert {
                generation: self.library_generation(),
                photo,
                round: cert.round.clone(),
                original: Arc::new(original),
                expected: Arc::new(expected),
                source: cert.source.clone(),
                content: crate::media::content_key(p),
                source_evidence: Some(cert.source_evidence.clone()),
                eligible: true,
                reverted: false,
                later_after_revert: false,
                later_edits: false,
                record: Some((cert.conversation_epoch, cert.record)),
                certificate: Some(cert),
            }),
        );
        Ok(())
    }
    /// Only live trusted candidates can acquire/refresh an anchor after both saves.
    /// Load never calls this before validating the previous certificate.
    pub(crate) fn seal_companion_reverts(&mut self) -> Result<()> {
        let Some(lib) = self.library.as_ref() else { return Ok(()) };
        if !self.pending_log.is_empty() || self.interaction.is_some() || lib.last_error.is_some() || lib.unsaved_error.is_some() {
            return Ok(());
        }
        let seq = lib.journal().seq();
        let proofs: Vec<_> = self.mutation_guard.reverts.values().filter(|p| self.runtime_revert_current(p)).cloned().collect();
        let mut failed = None;
        for proof in proofs {
            match self.seal_companion_revert(&proof, seq) {
                Ok(cert) => {
                    self.companion_archive.certificate_dirty.remove(&proof.photo);
                    if let Some(cert) = cert
                        && let Some(current) = self.mutation_guard.reverts.get_mut(&proof.photo)
                    {
                        Arc::make_mut(current).certificate = Some(cert);
                    }
                }
                Err(e) => {
                    if self.mutation_guard.reverts.get(&proof.photo).is_some_and(|p| p.eligible) {
                        self.companion_archive.certificate_failed(proof.photo, e.to_string());
                        failed =
                            Some(crate::EngineError::Other("The AI result is kept, but its revert certificate was not saved. Retry saving.".into()));
                    } else {
                        failed = Some(e);
                    }
                }
            }
        }
        failed.map_or(Ok(()), Err)
    }
    fn seal_companion_revert(&mut self, proof: &RuntimeRevert, seq: u64) -> Result<Option<Certificate>> {
        if proof.certificate.as_ref().is_some_and(|c| c.core_seq == seq) {
            return Ok(None);
        }
        if self.companion_history_unsaved(proof.photo) {
            return Ok(None);
        }
        let Some(h) = self.companion_photo_header(proof.photo)? else { return Ok(None) };
        let Some((epoch, ordinal)) = proof.record else { return Ok(None) };
        if epoch != h.conversation_epoch {
            return Ok(None);
        }
        let Some(record) = self.certificate_record(proof.photo, ordinal)? else { return Ok(None) };
        if record.local_round_id != proof.round.as_str() || record.original != *proof.original || record.recent_complete_result != *proof.expected {
            return Ok(None);
        }
        let Some(source_evidence) = proof.source_evidence.clone() else { return Ok(None) };
        if self.fresh_revert_source(&proof.source).ok().as_deref() != Some(source_evidence.as_str()) {
            self.invalidate_companion_reverts_before(&[proof.photo].into_iter().collect())?;
            return Ok(None);
        }
        let lib = self.library.as_mut().ok_or_else(|| error("library closed"))?;
        let mut guard = read_guard(lib.files.as_mut(), proof.photo)?.unwrap_or_else(|| json!({}));
        let epoch = guard["mutation_epoch"].as_u64().unwrap_or(0);
        if proof.certificate.as_ref().is_some_and(|c| c.mutation_epoch != epoch || !guard["certificate"]["eligible"].as_bool().unwrap_or(false)) {
            return Ok(None);
        }
        let Some(storage_identity) = identity(lib.files.as_mut(), true)? else { return Ok(None) };
        let cert = Certificate {
            version: 1,
            photo: proof.photo,
            round: proof.round.clone(),
            storage_identity,
            mutation_epoch: epoch,
            conversation_epoch: h.conversation_epoch,
            core_seq: seq,
            record: ordinal,
            original_digest: digest(&proof.original)?,
            result_digest: digest(&proof.expected)?,
            source: proof.source.clone(),
            source_evidence,
            eligible: true,
        };
        if proof.certificate.as_ref().is_some_and(|c| c.core_seq == seq && c.conversation_epoch == h.conversation_epoch) {
            return Ok(None);
        }
        guard["mutation_epoch"] = json!(epoch);
        guard["certificate"] = json!(cert);
        let _ = seal_pending(lib.files.as_mut(), proof.photo)?;
        let stamp = json!({"version":1,"photo":proof.photo.0,"certificate_digest":lightcraft_preview::hash_bytes(&serde_json::to_vec(&cert).map_err(error)?).to_string()});
        lib.files.write_atomic(&seal_key(proof.photo), &serde_json::to_vec(&stamp).map_err(error)?).map_err(error)?;
        write_guard(lib.files.as_mut(), proof.photo, &guard)?;
        // Only removal after a successful guard write can retire the intent.
        // A guard-write error after visible rename leaves cold authority blocked.
        lib.files.remove(&seal_key(proof.photo)).map_err(error)?;
        Ok(Some(cert))
    }
}
