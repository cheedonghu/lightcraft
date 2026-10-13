//! Runtime ownership of a task photo. Tokens never come from JSON or labels.
//! Mutation preflight is the extension point for the durable invalidation gate.
use super::LocalRoundId;
use crate::{EngineError, Result, Session};
use lightcraft_catalog::{Op, PhotoId, Source};
use lightcraft_develop::DevelopSettings;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Weak};

#[derive(Debug)]
struct Lease {
    generation: u64,
    photo: PhotoId,
    round: LocalRoundId,
    original: Arc<DevelopSettings>,
    source: Source,
    source_evidence: Option<String>,
}

/// Holding this non-serializable capability protects the photo. Dropping it releases
/// ownership, including a failed local enqueue and a safely retired job.
#[derive(Debug)]
pub struct LeaseToken(Arc<Lease>);

#[derive(Default)]
pub(crate) struct MutationGuard {
    active: Weak<Lease>,
    pub(super) reverts: BTreeMap<PhotoId, Arc<RuntimeRevert>>,
    pub(super) loaded: BTreeSet<PhotoId>,
    pub(crate) loaded_seq: u64,
    pub(crate) clean_load: bool,
    pub(crate) load_fenced: bool,
    pub(crate) retire_load: bool,
    pub(crate) rejected_interaction: bool,
}

/// A runtime publication proof, never constructed from saved JSON or parameters.
#[derive(Clone, Debug)]
pub(super) struct RuntimeRevert {
    pub(super) generation: u64,
    pub(super) photo: PhotoId,
    pub(super) round: LocalRoundId,
    pub(super) original: Arc<DevelopSettings>,
    pub(super) expected: Arc<DevelopSettings>,
    pub(super) source: Source,
    pub(super) content: String,
    pub(super) source_evidence: Option<String>,
    pub(super) certificate: Option<super::revert_guard::Certificate>,
    pub(super) record: Option<(u64, u64)>,
    pub(super) eligible: bool,
    pub(super) reverted: bool,
    pub(super) later_after_revert: bool,
    pub(super) later_edits: bool,
}
/// Opaque permission bound to this exact runtime proof and photo version.
/// Callers cannot create or serialize it; commit rechecks it before restoring.
pub struct VerifiedRevert {
    proof: Arc<RuntimeRevert>,
    revision: u64,
}

/// Historical display state of a round; this is not a permission token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundRevertState {
    Unverified,
    Eligible,
    LaterEdits,
    Reverted,
    RevertedWithLaterEdits,
}

impl Session {
    pub fn companion_task_photo(&self) -> Option<PhotoId> {
        self.mutation_guard.active.upgrade().filter(|l| l.generation == self.library_generation()).map(|l| l.photo)
    }

    pub fn acquire_companion_lease(&mut self, photo: PhotoId, round: LocalRoundId) -> Result<LeaseToken> {
        if self.companion_task_photo().is_some() {
            return Err(EngineError::Other("Another AI task is active; stop it or wait for completion.".into()));
        }
        let p = self.catalog.photo(photo).ok_or(lightcraft_catalog::CatalogError::NoPhoto(photo))?;
        if p.deleted {
            return Err(EngineError::Other("This photo is unavailable for AI processing.".into()));
        }
        // Finish the user's click-time editing transaction before freezing its start.
        self.end_interaction()?;
        let original = self.develop_of(photo).ok_or(lightcraft_catalog::CatalogError::NoPhoto(photo))?;
        let source = self.catalog.photo(photo).ok_or(lightcraft_catalog::CatalogError::NoPhoto(photo))?.source.clone();
        let source_evidence = self.fresh_revert_source(&source).ok();
        let lease = Arc::new(Lease { generation: self.library_generation(), photo, round, original, source, source_evidence });
        self.mutation_guard.active = Arc::downgrade(&lease);
        Ok(LeaseToken(lease))
    }

    /// Validate the complete final operation before any catalog, file or history effects.
    /// Ordinary rating/flag/metadata operations retain their existing policy.
    pub fn preflight_mutation(&self, op: &Op) -> Result<()> {
        let Some(photo) = self.companion_task_photo() else { return Ok(()) };
        let mut pending = vec![op];
        while let Some(op) = pending.pop() {
            match op {
                Op::Batch { ops } => pending.extend(ops),
                Op::SetDevelop { id, .. }
                | Op::RemovePhoto { id }
                | Op::SetDeleted { id, .. }
                | Op::SetFile { id, .. }
                | Op::Relink { id, .. }
                | Op::SetContent { id, .. }
                | Op::SetEmbeddedLens { id, .. }
                | Op::SetAsShotWb { id, .. }
                | Op::SetHistory { id, .. }
                | Op::PushHistory { id, .. }
                | Op::SetVersions { id, .. }
                | Op::SetLocal { id, .. }
                    if *id == photo =>
                {
                    return Err(EngineError::Other("Manual editing is paused while AI processes this photo; stop it or wait for completion.".into()));
                }
                Op::AddPhoto { photo: p } if p.id == photo => {
                    return Err(EngineError::Other("Manual editing is paused while AI processes this photo; stop it or wait for completion.".into()));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The mutable authorization seam for future durable safety records. All manual
    /// mutation paths call this before parameters, files or Undo/Redo stacks change.
    /// Lease inspection remains pure; adding durable invalidation here never requires
    /// weakening the lease guard or moving an authorization after a side effect.
    pub fn authorize_mutation(&mut self, op: &Op) -> Result<()> {
        self.preflight_mutation(op)?;
        let mut pending = vec![op];
        let mut changed = Vec::new();
        while let Some(part) = pending.pop() {
            match part {
                Op::Batch { ops } => pending.extend(ops),
                Op::SetDevelop { id, settings, label, .. } => {
                    if label.starts_with("History:") || self.develop_of(*id).is_none_or(|current| current != *settings) {
                        changed.push(*id);
                    }
                }
                Op::SetFile { id, source, .. } | Op::Relink { id, source, .. } if self.catalog.photo(*id).is_none_or(|p| p.source != *source) => {
                    changed.push(*id);
                }
                Op::SetDeleted { id, deleted } if self.catalog.photo(*id).is_none_or(|p| p.deleted != *deleted) => {
                    changed.push(*id);
                }
                Op::RemovePhoto { id } => {
                    changed.push(*id);
                }
                Op::AddPhoto { photo } if self.catalog.photo(photo.id).is_some() => {
                    changed.push(photo.id);
                }
                Op::SetContent { id, width, height, file_size, content_hash, preview_only }
                    if self.catalog.photo(*id).is_none_or(|p| {
                        p.width != *width
                            || p.height != *height
                            || p.file_size != *file_size
                            || p.content_hash != *content_hash
                            || p.preview_only != *preview_only
                    }) =>
                {
                    changed.push(*id);
                }
                Op::SetEmbeddedLens { id, lens } if self.catalog.photo(*id).is_none_or(|p| p.embedded_lens.as_ref() != lens.as_deref()) => {
                    changed.push(*id);
                }
                _ => {}
            }
        }
        if self.mutation_guard.rejected_interaction && self.interaction.as_ref().is_some_and(|i| changed.contains(&i.photo)) {
            return Err(EngineError::Other("This gesture was not applied. Release it, then retry the edit explicitly.".into()));
        }
        let changed = changed.into_iter().collect();
        if let Err(e) = self.invalidate_companion_reverts_before(&changed) {
            if self.interaction.as_ref().is_some_and(|i| changed.contains(&i.photo)) {
                self.mutation_guard.rejected_interaction = true;
            }
            return Err(e);
        }
        Ok(())
    }

    pub(crate) fn note_companion_actual_edit(&mut self, op: &Op) {
        let mut pending = vec![op];
        while let Some(part) = pending.pop() {
            match part {
                Op::Batch { ops } => pending.extend(ops),
                Op::SetDevelop { id, settings, label, .. } => {
                    if let Some(p) = self.mutation_guard.reverts.get_mut(id)
                        && (label.starts_with("History:") || *settings != p.expected)
                    {
                        let p = Arc::make_mut(p);
                        p.later_edits = true;
                        p.later_after_revert |= p.reverted;
                    }
                }
                Op::SetFile { id, source, .. } | Op::Relink { id, source, .. } => {
                    if let Some(p) = self.mutation_guard.reverts.get_mut(id)
                        && *source != p.source
                    {
                        let p = Arc::make_mut(p);
                        p.later_edits = true;
                        p.later_after_revert |= p.reverted;
                    }
                }
                _ => {}
            }
        }
    }
    /// Only a live round's private capability may publish a single-photo develop result.
    /// Revision and round identity are checked again immediately before publication.
    pub fn commit_companion(&mut self, token: &LeaseToken, round: &LocalRoundId, revision: u64, op: Op) -> Result<()> {
        let active = self.mutation_guard.active.upgrade().ok_or_else(|| EngineError::Other("AI task ownership has ended.".into()))?;
        if !Arc::ptr_eq(&active, &token.0)
            || active.generation != self.library_generation()
            || &active.round != round
            || self.photo_revision(active.photo) != revision
        {
            return Err(EngineError::Other("AI result is stale; the photo was not changed.".into()));
        }
        let mut pending = vec![&op];
        while let Some(part) = pending.pop() {
            match part {
                Op::Batch { ops } => pending.extend(ops),
                Op::SetDevelop { id, .. } | Op::PushHistory { id, .. } if *id == active.photo => {}
                _ => return Err(EngineError::Other("AI publication may only update its task photo.".into())),
            }
        }
        if self.catalog.photo(active.photo).is_none_or(|p| p.source != active.source)
            || active.source_evidence.as_deref().is_some_and(|evidence| self.fresh_revert_source(&active.source).ok().as_deref() != Some(evidence))
        {
            return Err(EngineError::Other(
                "The original source could not be verified or has changed. Use ordinary History; the current photo was kept.".into(),
            ));
        }
        let before = self.develop_of(active.photo).ok_or(lightcraft_catalog::CatalogError::NoPhoto(active.photo))?;
        let mut proposed = vec![&op];
        let mut changes = false;
        while let Some(part) = proposed.pop() {
            match part {
                Op::Batch { ops } => proposed.extend(ops),
                Op::SetDevelop { settings, .. } => changes |= *settings != before,
                _ => {}
            }
        }
        if changes {
            self.invalidate_companion_reverts_before(&BTreeSet::from([active.photo]))?;
        }
        self.commit_preflighted("AI edit plan", op)?;
        let p = self.catalog.photo(active.photo).ok_or(lightcraft_catalog::CatalogError::NoPhoto(active.photo))?;
        if p.develop == before {
            return Ok(());
        }
        self.mutation_guard.reverts.insert(
            active.photo,
            Arc::new(RuntimeRevert {
                generation: active.generation,
                photo: active.photo,
                round: active.round.clone(),
                original: active.original.clone(),
                expected: p.develop.clone(),
                source: p.source.clone(),
                content: crate::media::content_key(p),
                source_evidence: active.source_evidence.clone(),
                certificate: None,
                record: None,
                eligible: true,
                reverted: false,
                later_after_revert: false,
                later_edits: false,
            }),
        );
        Ok(())
    }
    /// Cheap runtime display query, independent of uploaded content and UI caches.
    pub fn companion_round_revert_state(&self, photo: PhotoId, round: &LocalRoundId) -> RoundRevertState {
        let Some(proof) = self.mutation_guard.reverts.get(&photo).filter(|p| p.generation == self.library_generation()) else {
            return RoundRevertState::Unverified;
        };
        if &proof.round != round {
            // A different identity may be a newer no-op or an older result.
            // This one current proof has no ordering fact about that round.
            return RoundRevertState::Unverified;
        }
        if proof.reverted && proof.later_after_revert {
            RoundRevertState::RevertedWithLaterEdits
        } else if proof.reverted {
            RoundRevertState::Reverted
        } else if self.runtime_revert_current(proof) {
            RoundRevertState::Eligible
        } else if proof.later_edits {
            RoundRevertState::LaterEdits
        } else {
            RoundRevertState::Unverified
        }
    }
    /// Verify an explicit round, including fresh original source evidence.
    pub fn verify_round_revert(&mut self, photo: PhotoId, round: &LocalRoundId) -> Result<VerifiedRevert> {
        self.prepare_companion_revert(photo)?;
        self.preflight_mutation(&Op::SetDevelop {
            id: photo,
            settings: Arc::new(DevelopSettings::default()),
            label: "Undo AI round".into(),
            edited: None,
        })?;
        let proof = self
            .mutation_guard
            .reverts
            .get(&photo)
            .filter(|p| &p.round == round)
            .cloned()
            .ok_or_else(|| EngineError::Other("This round has no verified revert permission; use ordinary History.".into()))?;
        if !self.runtime_revert_current(&proof) {
            let message = if proof.later_edits {
                "The photo has later edits; use ordinary History. The current photo was kept."
            } else {
                "This round has no verified revert permission; use ordinary History."
            };
            return Err(EngineError::Other(message.into()));
        }
        self.verify_revert_source(&proof)?;
        Ok(VerifiedRevert { proof, revision: self.photo_revision(photo) })
    }
    fn verify_revert_source(&mut self, proof: &RuntimeRevert) -> Result<()> {
        if proof.source_evidence.as_deref().is_none_or(|evidence| self.fresh_revert_source(&proof.source).ok().as_deref() != Some(evidence)) {
            self.invalidate_companion_reverts_before(&[proof.photo].into_iter().collect())?;
            return Err(EngineError::Other(
                "The original source could not be verified or has changed. Use ordinary History; the current photo was kept.".into(),
            ));
        }
        Ok(())
    }
    pub(super) fn runtime_revert_current(&self, proof: &RuntimeRevert) -> bool {
        proof.eligible
            && proof.source_evidence.is_some()
            && proof.generation == self.library_generation()
            && self.catalog.photo(proof.photo).is_some_and(|p| {
                !p.deleted && p.source == proof.source && crate::media::content_key(p) == proof.content && *p.develop == *proof.expected
            })
    }
    /// One undoable single-photo restoration. Auto Sync is never expanded, and
    /// neither current consent nor a network connection is required.
    pub fn commit_verified_round_revert(&mut self, verified: VerifiedRevert) -> Result<()> {
        let p = &verified.proof;
        if self.photo_revision(p.photo) != verified.revision
            || !self.mutation_guard.reverts.get(&p.photo).is_some_and(|current| Arc::ptr_eq(current, p))
            || !self.runtime_revert_current(p)
        {
            return Err(EngineError::Other("The round revert permission is stale; the photo was kept.".into()));
        }
        self.verify_revert_source(p)?;
        let op = self.develop_op(p.photo, (*p.original).clone(), "Undo AI round").ok_or(lightcraft_catalog::CatalogError::NoPhoto(p.photo))?;
        self.commit("Undo AI round", op)?;
        if let Some(current) = self.mutation_guard.reverts.get_mut(&p.photo) {
            let current = Arc::make_mut(current);
            current.eligible = false;
            current.reverted = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Selection;
    use serde_json::json;

    #[test]
    fn protects_actual_targets_whole_batches_and_history_without_label_bypass() {
        let mut s = Session::with_demo();
        let a = s.active().unwrap();
        let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
        let original = s.develop_of(a).unwrap();
        let mut edit = (*original).clone();
        edit.light.exposure = 0.5;
        s.set_develop(a, edit.clone(), "before task").unwrap();
        let undo = s.undo.len();
        let round = LocalRoundId::new().unwrap();
        let token = s.acquire_companion_lease(a, round.clone()).unwrap();
        assert!(s.acquire_companion_lease(b, LocalRoundId::new().unwrap()).is_err());
        assert!(s.execute("develop.set", &json!({"id":b.0,"values":{"light.exposure":1.0}})).is_err());
        assert!(s.set_develop(a, (*original).clone(), "AI edit plan").is_err());
        assert!(s.apply_system(Op::SetDevelop { id: a, settings: original.clone(), label: "AI edit plan".into(), edited: None }).is_err());
        assert!(s.undo_step().is_err());
        assert_eq!(s.undo.len(), undo);
        s.selection = Selection { ids: vec![b, a], active: Some(b) };
        s.auto_sync = true;
        let before = s.develop_of(b).unwrap();
        edit.light.exposure = 1.0;
        assert!(s.set_develop(b, edit.clone(), "Auto Sync").is_err());
        assert_eq!(s.develop_of(b).unwrap(), before);
        s.begin_interaction("Exposure").unwrap();
        assert!(s.set_develop(b, edit.clone(), "Exposure").is_err());
        s.cancel_interaction().unwrap();
        assert!(s.interaction.is_none());
        let op = s.develop_op(a, (*original).clone(), "AI edit plan").unwrap();
        assert!(s.commit_companion(&token, &LocalRoundId::new().unwrap(), s.photo_revision(a), op.clone()).is_err());
        s.commit_companion(&token, &round, s.photo_revision(a), op).unwrap();
        drop(token);
        s.selection = Selection::single(a);
        s.auto_sync = false;
        s.undo_step().unwrap();
        let token = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
        let redo = s.redo.len();
        assert!(s.redo_step().is_err());
        assert_eq!(s.redo.len(), redo);
        drop(token);
        s.redo_step().unwrap();
    }
    #[test]
    fn file_and_folder_batches_and_undo_reject_before_disk_changes() {
        let root = std::env::temp_dir().join(format!("lc-ux03-files-{}", LocalRoundId::new().unwrap().as_str()));
        let dir = root.join("original");
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Session::with_demo();
        let a = s.active().unwrap();
        let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
        for (id, name) in [(a, "a.png"), (b, "b.png")] {
            let path = dir.join(name);
            let img = lightcraft_raster::Rgba8::from_fn(8, 8, |x, y| [(x * 20) as u8, (y * 20) as u8, 64, 255]);
            let bytes =
                crate::export::encode_image(&img, &crate::export::ExportOptions { format: crate::export::ExportFormat::Png, ..Default::default() })
                    .unwrap();
            std::fs::write(&path, &bytes).unwrap();
            s.commit(
                "setup",
                Op::SetFile { id, file_name: name.into(), source: lightcraft_catalog::Source::File { path: path.to_string_lossy().into() } },
            )
            .unwrap();
        }
        let token = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
        let plans = s.plan_rename(&[b, a], "renamed-{seq}", 1);
        let undo = s.undo.len();
        let revision = s.catalog.revision;
        assert!(s.apply_rename(&plans).is_err());
        assert_eq!(s.catalog.revision, revision);
        assert_eq!(s.undo.len(), undo);
        assert!(dir.join("a.png").exists());
        assert!(dir.join("b.png").exists());
        assert!(crate::cmd::browse::move_folder(&mut s, &dir.to_string_lossy(), &root.join("moved").to_string_lossy()).is_err());
        assert!(dir.is_dir());
        assert!(!root.join("moved").exists());
        drop(token);
        s.apply_rename(&plans).unwrap();
        let token = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
        let log = s.pending_log.len();
        let undo = s.undo.len();
        assert!(s.undo_step().is_err());
        assert_eq!(s.undo.len(), undo);
        assert_eq!(s.pending_log.len(), log);
        assert!(!dir.join("a.png").exists());
        assert!(!dir.join("b.png").exists());
        drop(token);
        s.undo_step().unwrap();
        assert!(dir.join("a.png").exists());
        assert!(dir.join("b.png").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn silent_preview_metadata_independent_edits_and_local_eviction_use_same_lease() {
        let mut s = Session::with_demo();
        let a = s.active().unwrap();
        let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
        let token = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
        assert!(s.apply_silent(Op::SetDevelop { id: a, settings: Arc::new(Default::default()), label: "preview".into(), edited: None }).is_err());
        s.execute("photo.rate", &json!({"ids":[a.0],"rating":3})).unwrap();
        s.selection = Selection::single(b);
        let mut edit = (*s.develop_of(b).unwrap()).clone();
        edit.light.exposure = 0.4;
        s.set_develop(b, edit, "independent").unwrap();
        drop(token);
        // An otherwise evictable task photo stays in use after browsing away.
        let mut s = Session::new();
        s.clock = Box::new(|| "2026-10-13T12:00:00".into());
        let a = PhotoId(1);
        let mut p = lightcraft_catalog::Photo::new(
            a,
            lightcraft_catalog::Source::File { path: "/ux03/archive/a.png".into() },
            "a.png",
            "PNG",
            8,
            8,
            "2026-01-01T12:00:00",
        );
        p.local = true;
        p.set_local_baseline();
        s.catalog.apply(Op::AddPhoto { photo: Box::new(p) }).unwrap();
        s.catalog.apply(Op::SetBrowsed { folder: "/ux03/archive".into(), at: Some("2026-01-01T12:00:00".into()) }).unwrap();
        let token = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
        assert!(s.forget_local(false, Some(1)).evict.is_empty());
        assert!(s.catalog.photo(a).is_some());
        drop(token);
        assert_eq!(s.forget_local(false, Some(1)).evict, vec![a]);
        assert!(s.catalog.photo(a).is_none());
    }
    #[test]
    fn auto_sync_enabled_mid_preview_rejects_end_before_restoring_or_taking_interaction() {
        let mut s = Session::with_demo();
        let a = s.active().unwrap();
        let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
        let a_start = s.develop_of(a).unwrap();
        let b_start = s.develop_of(b).unwrap();
        let _token = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
        s.selection = Selection::single(b);
        s.begin_interaction("Exposure").unwrap();
        let mut edit = (*b_start).clone();
        edit.light.exposure = 0.7;
        s.set_develop(b, edit.clone(), "Exposure").unwrap();
        let preview = s.develop_of(b).unwrap();
        s.auto_sync = true;
        s.selection = Selection { ids: vec![b, a], active: Some(b) };
        let undo = s.undo.len();
        let redo = s.redo.len();
        let log = s.pending_log.len();
        assert!(s.end_interaction().is_err());
        assert_eq!(s.develop_of(b).unwrap(), preview);
        assert_eq!(s.develop_of(a).unwrap(), a_start);
        assert_eq!(s.interaction.as_ref().unwrap().photo, b);
        assert_eq!(s.undo.len(), undo);
        assert_eq!(s.redo.len(), redo);
        assert_eq!(s.pending_log.len(), log);
        // Cancellation is independent: it restores B alone without applying Auto Sync.
        s.cancel_interaction().unwrap();
        assert_eq!(s.develop_of(b).unwrap(), b_start);
        s.auto_sync = false;
        s.begin_interaction("Exposure").unwrap();
        s.auto_sync = true;
        assert!(s.set_develop(b, edit, "Exposure").is_err());
        assert_eq!(s.develop_of(b).unwrap(), b_start);
        assert_eq!(s.develop_of(a).unwrap(), a_start);
        s.cancel_interaction().unwrap();
    }
    #[test]
    fn excluded_auto_sync_fields_finish_an_independent_interaction() {
        let mut s = Session::with_demo();
        let a = s.active().unwrap();
        let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
        let a_start = s.develop_of(a).unwrap();
        let _token = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
        s.selection = Selection { ids: vec![b, a], active: Some(b) };
        s.auto_sync = true;
        s.begin_interaction("Independent process").unwrap();
        let mut edit = (*s.develop_of(b).unwrap()).clone();
        edit.process = lightcraft_develop::ProcessVersion::legacy();
        // Process changes are excluded from Auto Sync, so only B's actual Op is guarded.
        s.set_develop(b, edit.clone(), "Independent process").unwrap();
        s.end_interaction().unwrap();
        assert_eq!(*s.develop_of(b).unwrap(), edit);
        assert_eq!(s.develop_of(a).unwrap(), a_start);
        assert!(s.interaction.is_none());
        s.undo_step().unwrap();
        assert_eq!(s.develop_of(a).unwrap(), a_start);
    }
}
