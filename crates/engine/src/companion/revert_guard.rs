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
const CORE: &str = "companion/v2/core-proof.json";
const CORE_PENDING: &str = "companion/v2/core-proof-pending.json";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct CoreProof {
    version: u32,
    storage_identity: String,
    chain: LocalRoundId,
    first_seq: u64,
    core_seq: u64,
}
#[derive(Serialize, Deserialize)]
struct Association {
    version: u32,
    photo: PhotoId,
    storage_identity: String,
    chain: LocalRoundId,
    core_seq: u64,
    certificate_digest: String,
}
fn association_key(id: &LocalRoundId) -> Result<String> {
    if !id.as_str().strip_prefix("local-").is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(error("invalid revert association"));
    }
    Ok(format!("companion/v2/revert-associations/{}.json", id.as_str()))
}
fn certificate_digest(cert: &Certificate) -> Result<String> {
    Ok(lightcraft_preview::hash_bytes(&serde_json::to_vec(cert).map_err(error)?).to_string())
}
fn core_proof(store: &mut dyn Store) -> Result<Option<CoreProof>> {
    let Some(bytes) = store.read_limited(CORE, LIMIT).map_err(error)? else { return Ok(None) };
    let proof: CoreProof = serde_json::from_slice(&bytes).map_err(error)?;
    if proof.version != 1 || proof.first_seq > proof.core_seq {
        return Err(error("unknown core continuity proof"));
    }
    Ok(Some(proof))
}
fn intent_bytes() -> Result<Vec<u8>> {
    // Cleanup debt survives archive drops in this process. Each newer critical
    // intent must differ even when its journal sequence/settings are unchanged.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let attempt = NEXT
        .try_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| error("companion intent counter exhausted"))?;
    serde_json::to_vec(&json!({"version":1,"pending":true,"attempt":attempt})).map_err(error)
}
fn core_pending(store: &mut dyn Store) -> Result<bool> {
    let Some(bytes) = store.read_limited(CORE_PENDING, LIMIT).map_err(error)? else { return Ok(false) };
    let value: Value = serde_json::from_slice(&bytes).map_err(error)?;
    if value["version"] != 1 || value["pending"] != true {
        return Err(error("unknown core continuity intent"));
    }
    Ok(true)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Certificate {
    version: u32,
    photo: PhotoId,
    round: LocalRoundId,
    storage_identity: String,
    mutation_epoch: u64,
    conversation_epoch: u64,
    core_seq: u64,
    #[serde(default)]
    association: Option<LocalRoundId>,
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
#[derive(Deserialize)]
struct StorageIdentity {
    version: u32,
    identity: String,
}
fn identity(store: &mut dyn Store, create: bool) -> Result<Option<String>> {
    if let Some(bytes) = store.read_limited(IDENTITY, LIMIT).map_err(error)? {
        // Typed owning fields reject duplicates instead of last-key-wins authority.
        let value: StorageIdentity = serde_json::from_slice(&bytes).map_err(error)?;
        if value.version != 1 || value.identity.is_empty() {
            return Err(error("unknown companion storage identity protected"));
        }
        return Ok(Some(value.identity));
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
pub(crate) struct LoadProtection {
    pub(crate) fenced: bool,
    pub(crate) uncertain: bool,
    pub(crate) protected_reason: Option<String>,
}
pub(crate) fn begin_load(store: &mut dyn Store) -> Result<LoadProtection> {
    let pending = load_pending(store)?;
    let failed_close = close_pending(store)? == Some(true) || core_pending(store)?;
    let storage_identity = identity(store, false);
    if pending || storage_identity.as_ref().is_ok_and(|identity| identity.is_some()) || storage_identity.is_err() {
        // A malformed identity must remain available for diagnosis. The separate
        // durable load fence denies every old certificate before core replay.
        // If this write fails, opening still fails: memory is not a safe fence.
        store.write_atomic(LOAD, br#"{"version":1,"pending":true}"#).map_err(error)?;
        return Ok(LoadProtection {
            fenced: true,
            uncertain: pending || failed_close || storage_identity.is_err(),
            protected_reason: storage_identity.err().map(|reason| reason.to_string()),
        });
    }
    Ok(LoadProtection { fenced: false, uncertain: false, protected_reason: None })
}
impl Session {
    /// Validate the previous acknowledged core anchor before any new save. A
    /// mismatch retires the old lineage, never rebinds it to loaded equality.
    pub(crate) fn load_companion_core_proof(&mut self) {
        let Some(lib) = self.library.as_mut() else { return };
        let stored = core_proof(lib.files.as_mut());
        match stored {
            Ok(Some(proof))
                if self.mutation_guard.clean_load
                    && proof.core_seq == self.mutation_guard.loaded_seq
                    && identity(lib.files.as_mut(), false).ok().flatten().as_deref() == Some(proof.storage_identity.as_str()) =>
            {
                self.mutation_guard.core_proof = Some(proof);
            }
            Ok(None) if !self.mutation_guard.load_fenced => {}
            _ => {
                self.mutation_guard.clean_load = false;
                self.mutation_guard.retire_load = self.mutation_guard.load_fenced;
            }
        }
    }
    pub(crate) fn begin_companion_core_save(&mut self) -> Result<()> {
        if self.mutation_guard.core_proof.is_some() {
            let Some(lib) = self.library.as_mut() else { return Ok(()) };
            if let Err(e) = lib.files.write_atomic(CORE_PENDING, &intent_bytes()?) {
                self.companion_archive.core_save_failed(e.to_string());
                return Err(error(e));
            }
        }
        Ok(())
    }
    /// Constant owning metadata work. Only this session's validated chain can
    /// advance after acknowledged core and companion saves; cold certificates
    /// remain immutable and are checked by their exact association on action.
    pub(crate) fn advance_companion_core_proof(&mut self) -> Result<()> {
        let Some(mut proof) = self.mutation_guard.core_proof.clone() else { return Ok(()) };
        let Some(lib) = self.library.as_mut() else { return Ok(()) };
        let seq = lib.journal().seq();
        if proof.core_seq != seq {
            proof.core_seq = seq;
            if let Err(e) = lib.files.write_atomic(CORE, &serde_json::to_vec(&proof).map_err(error)?) {
                self.companion_archive.core_save_failed(e.to_string());
                return Err(error(e));
            }
            self.mutation_guard.core_proof = Some(proof);
        }
        let intent = (|| -> Result<Option<Vec<u8>>> {
            if !core_pending(lib.files.as_mut())? {
                return Ok(None);
            }
            lib.files.read_limited(CORE_PENDING, LIMIT).map_err(error)?.ok_or_else(|| error("core intent missing")).map(Some)
        })();
        let bytes = match intent {
            Ok(bytes) => bytes,
            Err(e) => {
                self.companion_archive.core_save_failed(e.to_string());
                return Err(e);
            }
        };
        if let Some(bytes) = bytes
            && let Err(e) = lib.files.remove(CORE_PENDING)
        {
            // Core and its continuity anchor are acknowledged. An uncertain
            // deletion is cleanup debt; a still-present intent stays conservative.
            self.companion_archive.marker_cleanup_failed(CORE_PENDING.into(), None, &bytes, e.to_string());
        }
        // The core log may already be acknowledged while this independent
        // continuity write is not. Clear its recovery state only after retry
        // acknowledges the owning metadata; marker deletion remains cleanup.
        self.companion_archive.core_save_error = None;
        Ok(())
    }
    fn core_proof_for_certificate(&mut self, seq: u64) -> Result<CoreProof> {
        if let Some(proof) = &self.mutation_guard.core_proof {
            return Ok(proof.clone());
        }
        let lib = self.library.as_mut().ok_or_else(|| error("library closed"))?;
        let storage_identity = identity(lib.files.as_mut(), true)?.ok_or_else(|| error("storage identity missing"))?;
        let proof = CoreProof { version: 1, storage_identity, chain: LocalRoundId::new().map_err(error)?, first_seq: seq, core_seq: seq };
        lib.files.write_atomic(CORE, &serde_json::to_vec(&proof).map_err(error)?).map_err(error)?;
        self.mutation_guard.core_proof = Some(proof.clone());
        Ok(proof)
    }
    fn valid_companion_association(&mut self, cert: &Certificate) -> Result<bool> {
        let Some(proof) = self.mutation_guard.core_proof.as_ref() else { return Ok(false) };
        let Some(id) = cert.association.as_ref() else { return Ok(false) };
        let Some(lib) = self.library.as_mut() else { return Ok(false) };
        let Some(bytes) = lib.files.read_limited(&association_key(id)?, LIMIT).map_err(error)? else { return Ok(false) };
        let a: Association = serde_json::from_slice(&bytes).map_err(error)?;
        Ok(a.version == 1
            && a.photo == cert.photo
            && a.storage_identity == cert.storage_identity
            && a.storage_identity == proof.storage_identity
            && a.chain == proof.chain
            && a.core_seq == cert.core_seq
            && a.core_seq >= proof.first_seq
            && a.core_seq <= proof.core_seq
            && a.certificate_digest == certificate_digest(cert)?)
    }

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
        if !self.mutation_guard.load_fenced || self.mutation_guard.protected_load {
            // Protected metadata and its acknowledged fence survive ordinary
            // core saves. Repair/reopen must not recover authority from equality.
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
        // An uncertain continuity transition has retired the old storage identity;
        // cleanup cannot reconnect its old associations to a new core sequence.
        if self.mutation_guard.core_proof.is_none() && core_pending(lib.files.as_mut())? {
            lib.files.remove(CORE_PENDING).map_err(error)?;
        }
        lib.files.remove(LOAD).map_err(error)?;
        self.mutation_guard.load_fenced = false;
        Ok(())
    }
    pub(crate) fn begin_companion_close(&mut self) -> Result<()> {
        if self.mutation_guard.protected_load {
            // Opening acknowledged a persistent load fence and denied all AI
            // authority. Keep it through closing instead of rewriting bad identity.
            return Ok(());
        }
        let live = self.mutation_guard.reverts.values().any(|p| p.eligible);
        let Some(lib) = self.library.as_mut() else { return Ok(()) };
        if live || identity(lib.files.as_mut(), false)?.is_some() {
            // A durable fence rejects certificates until the final snapshot and
            // companion flush both finish. It also protects already sealed proofs.
            let _ = close_pending(lib.files.as_mut())?;
            lib.files.write_atomic(CLOSE, &intent_bytes()?).map_err(error)?;
        }
        Ok(())
    }
    pub(crate) fn finish_companion_close(&mut self) -> Result<()> {
        let Some(lib) = self.library.as_mut() else { return Ok(()) };
        if close_pending(lib.files.as_mut())?.is_some() {
            let bytes = lib.files.read_limited(CLOSE, LIMIT).map_err(error)?.ok_or_else(|| error("close intent missing"))?;
            if let Err(e) = lib.files.remove(CLOSE) {
                // This runs only after final snapshot, data, certificate and
                // continuity acknowledgments. Report saved plus cleanup debt.
                self.companion_archive.marker_cleanup_failed(CLOSE.into(), None, &bytes, e.to_string());
            }
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
            || seal_pending(lib.files.as_mut(), photo)?
            || close_pending(lib.files.as_mut())? == Some(true)
            || load_pending(lib.files.as_mut())?
            || identity(lib.files.as_mut(), false)?.as_deref() != Some(cert.storage_identity.as_str())
        {
            return Ok(());
        }
        if !self.valid_companion_association(&cert)? {
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
        let core = self.core_proof_for_certificate(seq)?;
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
            association: Some(proof.round.clone()),
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
        let association = Association {
            version: 1,
            photo: proof.photo,
            storage_identity: cert.storage_identity.clone(),
            chain: core.chain,
            core_seq: seq,
            certificate_digest: certificate_digest(&cert)?,
        };
        let id = cert.association.as_ref().ok_or_else(|| error("association missing"))?;
        guard["mutation_epoch"] = json!(epoch);
        guard["certificate"] = json!(cert);
        let _ = seal_pending(lib.files.as_mut(), proof.photo)?;
        let stamp = json!({"version":1,"photo":proof.photo.0,"certificate_digest":lightcraft_preview::hash_bytes(&serde_json::to_vec(&cert).map_err(error)?).to_string()});
        let stamp_bytes = serde_json::to_vec(&stamp).map_err(error)?;
        lib.files.write_atomic(&seal_key(proof.photo), &stamp_bytes).map_err(error)?;
        // Reuse this logical round's owning slot on every retry/reseal. Fence
        // first: replacing its association alone cannot qualify an old guard.
        lib.files.write_atomic(&association_key(id)?, &serde_json::to_vec(&association).map_err(error)?).map_err(error)?;
        write_guard(lib.files.as_mut(), proof.photo, &guard)?;
        // Association, complete certificate and both saves are acknowledged.
        // Marker retirement is now independent cleanup, not a failed certificate.
        // A guard-write error above still leaves cold authority fenced.
        if let Err(e) = lib.files.remove(&seal_key(proof.photo)) {
            self.companion_archive.marker_cleanup_failed(seal_key(proof.photo), Some(proof.photo), &stamp_bytes, e.to_string());
        }
        Ok(Some(cert))
    }
}
