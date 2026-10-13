//! Independently saveable editor metadata; conversation corruption cannot erase a draft.
use super::archive::error;
use crate::{Result, Session};
use lightcraft_catalog::{PhotoId, Store};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditorState {
    pub draft: String,
    pub scroll_offset: f32,
    pub follow_tail: bool,
}
impl Default for EditorState {
    fn default() -> Self {
        Self { draft: String::new(), scroll_offset: 0.0, follow_tail: true }
    }
}
#[derive(Default)]
pub(crate) struct Editors {
    values: BTreeMap<PhotoId, EditorState>,
    dirty: BTreeSet<PhotoId>,
    repair_payload: BTreeSet<PhotoId>,
    failure: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Header {
    version: u32,
    photo: PhotoId,
    draft_digest: String,
    draft_length: u64,
    scroll_offset: f32,
    follow_tail: bool,
}
fn name(photo: PhotoId) -> String {
    format!("companion/v2/photos/{}/editor.json", photo.0)
}
fn draft_name(photo: PhotoId, digest: &str) -> Result<String> {
    if lightcraft_preview::Hash128::parse(digest).is_none() {
        return Err(error("invalid draft identity"));
    }
    Ok(format!("companion/v2/photos/{}/draft-{digest}.txt", photo.0))
}
fn header(store: &mut dyn Store, photo: PhotoId) -> Result<Option<Header>> {
    let Some(bytes) = store.read_limited(&name(photo), 32 * 1024).map_err(error)? else { return Ok(None) };
    let h: Header = serde_json::from_slice(&bytes).map_err(error)?;
    if h.version != 1 || h.photo != photo || !h.scroll_offset.is_finite() || h.scroll_offset < 0.0 {
        return Err(error("invalid editor metadata"));
    }
    draft_name(photo, &h.draft_digest)?;
    Ok(Some(h))
}
fn read(store: &mut dyn Store, photo: PhotoId) -> Result<EditorState> {
    let Some(h) = header(store, photo)? else { return Ok(EditorState::default()) };
    let key = draft_name(photo, &h.draft_digest)?;
    let mut bytes = Vec::new();
    let mut offset = 0;
    let mut hash = lightcraft_preview::Hasher128::new();
    while offset < h.draft_length {
        let count = (h.draft_length - offset).min(128 * 1024) as usize;
        let data = store.read_range(&key, offset, count).map_err(error)?;
        if data.len() != count {
            return Err(error("draft is incomplete"));
        }
        bytes.try_reserve(data.len()).map_err(error)?;
        hash.update(&data);
        bytes.extend(data);
        offset += count as u64;
    }
    if hash.finish().to_string() != h.draft_digest {
        return Err(error("draft digest mismatch"));
    }
    Ok(EditorState { draft: String::from_utf8(bytes).map_err(error)?, scroll_offset: h.scroll_offset, follow_tail: h.follow_tail })
}
impl Editors {
    pub(crate) fn dirty(&self) -> bool {
        !self.dirty.is_empty()
    }
    pub(crate) fn unsaved(&self) -> Option<&str> {
        self.failure.as_deref().filter(|_| self.dirty())
    }
    pub(crate) fn persist(&mut self, store: &mut dyn Store, legacy: bool, blocked: Option<&str>) -> Result<()> {
        if !self.dirty() {
            return Ok(());
        }
        let result = (|| {
            if let Some(blocked) = blocked {
                return Err(error(blocked));
            }
            if legacy {
                return Err(error("draft retained in memory until legacy migration completes"));
            }
            super::storage::activate(store)?;
            let mut failure = None;
            for photo in self.dirty.iter().copied().collect::<Vec<_>>() {
                let Some(editor) = self.values.get(&photo) else { continue };
                let write = (|| {
                    let old = header(store, photo)?;
                    let digest = lightcraft_preview::hash_bytes(editor.draft.as_bytes()).to_string();
                    let key = draft_name(photo, &digest)?;
                    // The content object is immutable. A failed HEAD write leaves the prior draft readable.
                    if self.repair_payload.contains(&photo) || old.as_ref().is_none_or(|old| old.draft_digest != digest) {
                        store.write_atomic(&key, editor.draft.as_bytes()).map_err(error)?;
                    }
                    let h = Header {
                        version: 1,
                        photo,
                        draft_digest: digest.clone(),
                        draft_length: editor.draft.len() as u64,
                        scroll_offset: editor.scroll_offset,
                        follow_tail: editor.follow_tail,
                    };
                    store.write_atomic(&name(photo), &serde_json::to_vec(&h).map_err(error)?).map_err(error)?;
                    if let Some(old) = old.filter(|old| old.draft_digest != digest) {
                        // Only obsolete draft payloads, never conversation graphs. Cleanup failure is harmless.
                        if let Ok(key) = draft_name(photo, &old.draft_digest) {
                            let _ = store.remove(&key);
                        }
                    }
                    Ok(())
                })();
                match write {
                    Ok(()) => {
                        self.dirty.remove(&photo);
                        self.values.remove(&photo);
                        self.repair_payload.remove(&photo);
                    }
                    Err(e) => {
                        failure.get_or_insert(e);
                    }
                }
            }
            match failure {
                Some(e) => Err(e),
                None => Ok(()),
            }
        })();
        self.failure = result.as_ref().err().map(ToString::to_string);
        result
    }
}
impl Session {
    pub fn companion_editor(&mut self, photo: PhotoId) -> Result<EditorState> {
        if let Some(editor) = self.companion_archive.editors.values.get(&photo) {
            return Ok(editor.clone());
        }
        if let Some(blocked) = &self.companion_archive.blocked {
            return Err(error(blocked));
        }
        let loaded = match self.library.as_mut() {
            Some(library) => read(library.files.as_mut(), photo),
            None => Ok(EditorState::default()),
        };
        if loaded.is_err() {
            self.companion_archive.editors.repair_payload.insert(photo);
        }
        loaded
    }
    /// Stage without any storage I/O; the normal catalog save/retry/close path flushes every dirty photo.
    pub fn set_companion_editor(&mut self, photo: PhotoId, editor: EditorState) -> Result<()> {
        if self.catalog.photo(photo).is_none() {
            return Err(error("no such photo"));
        }
        if !editor.scroll_offset.is_finite() || editor.scroll_offset < 0.0 {
            return Err(error("invalid reading position"));
        }
        if self.companion_archive.editors.values.get(&photo) != Some(&editor) {
            self.companion_archive.editors.values.insert(photo, editor);
            self.companion_archive.editors.dirty.insert(photo);
        }
        Ok(())
    }
    pub fn companion_dirty_editors(&self) -> Vec<PhotoId> {
        self.companion_archive.editors.dirty.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::LibraryStores;
    use lightcraft_catalog::MemStore;
    use std::io;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    fn open(files: impl Store + 'static, catalog: MemStore) -> Session {
        let mut session = Session::new();
        session
            .open_library_in(LibraryStores { catalog: Box::new(catalog), files: Box::new(files), dir: "memory".into(), on_disk: false }, true)
            .unwrap();
        session
    }
    #[test]
    fn zero_history_drafts_save_reopen_and_survive_corrupt_inner_history() {
        let files = MemStore::new();
        let catalog = MemStore::new();
        let mut s = open(files.clone(), catalog.clone());
        let ids: Vec<_> = s.catalog.photos().take(2).map(|p| p.id).collect();
        let a = EditorState { draft: "A unsent 草稿\n".repeat(100), scroll_offset: 321.0, follow_tail: false };
        let b = EditorState { draft: "B unsent draft".into(), ..Default::default() };
        s.set_companion_editor(ids[0], a.clone()).unwrap();
        s.set_companion_editor(ids[1], b.clone()).unwrap();
        assert_eq!(s.companion_dirty_editors().len(), 2);
        s.persist().unwrap();
        assert!(s.companion_dirty_editors().is_empty());
        assert!(s.companion_photo_header(ids[0]).unwrap().is_none());
        files.set(&format!("companion/v2/photos/{}/HEAD.json", ids[0].0), b"broken inner conversation".to_vec());
        let mut reopened = open(files.clone(), catalog);
        assert_eq!(reopened.companion_editor(ids[0]).unwrap(), a);
        assert_eq!(reopened.companion_editor(ids[1]).unwrap(), b);
        let revised = EditorState { draft: "still independently saveable".into(), ..a };
        reopened.set_companion_editor(ids[0], revised.clone()).unwrap();
        reopened.persist().unwrap();
        assert_eq!(reopened.companion_editor(ids[0]).unwrap(), revised);
    }
    struct Failing {
        files: MemStore,
        fail: Arc<AtomicBool>,
        photo: PhotoId,
    }
    impl Store for Failing {
        fn read(&mut self, key: &str) -> io::Result<Option<Vec<u8>>> {
            self.files.read(key)
        }
        fn read_limited(&mut self, key: &str, limit: u64) -> io::Result<Option<Vec<u8>>> {
            self.files.read_limited(key, limit)
        }
        fn read_range(&mut self, key: &str, start: u64, length: usize) -> io::Result<Vec<u8>> {
            self.files.read_range(key, start, length)
        }
        fn write_atomic(&mut self, key: &str, bytes: &[u8]) -> io::Result<()> {
            if key == name(self.photo) && self.fail.load(Ordering::Relaxed) {
                return Err(io::Error::other("injected editor failure"));
            }
            self.files.write_atomic(key, bytes)
        }
        fn append(&mut self, key: &str, bytes: &[u8]) -> io::Result<()> {
            self.files.append(key, bytes)
        }
        fn remove(&mut self, key: &str) -> io::Result<()> {
            self.files.remove(key)
        }
        fn truncate(&mut self, key: &str, len: u64) -> io::Result<()> {
            self.files.truncate(key, len)
        }
        fn describe(&self) -> String {
            "injected editor store".into()
        }
    }
    #[test]
    fn failed_a_keeps_old_visible_draft_b_saves_and_retry_keeps_new_draft() {
        let files = MemStore::new();
        let catalog = MemStore::new();
        let mut s = open(files.clone(), catalog.clone());
        let ids: Vec<_> = s.catalog.photos().take(2).map(|p| p.id).collect();
        s.set_companion_editor(ids[0], EditorState { draft: "old A".into(), ..Default::default() }).unwrap();
        s.persist().unwrap();
        drop(s);
        let fail = Arc::new(AtomicBool::new(true));
        let mut s = open(Failing { files: files.clone(), fail: fail.clone(), photo: ids[0] }, catalog.clone());
        s.set_companion_editor(ids[0], EditorState { draft: "new A".into(), ..Default::default() }).unwrap();
        s.set_companion_editor(ids[1], EditorState { draft: "new B".into(), ..Default::default() }).unwrap();
        assert!(s.persist().is_err());
        assert!(s.unsaved().is_some());
        assert_eq!(s.companion_dirty_editors(), vec![ids[0]]);
        let mut reopened = open(files.clone(), catalog.clone());
        assert_eq!(reopened.companion_editor(ids[0]).unwrap().draft, "old A");
        assert_eq!(reopened.companion_editor(ids[1]).unwrap().draft, "new B");
        fail.store(false, Ordering::Relaxed);
        s.persist().unwrap();
        assert!(s.unsaved().is_none());
        let mut reopened = open(files, catalog);
        assert_eq!(reopened.companion_editor(ids[0]).unwrap().draft, "new A");
    }
    #[test]
    fn legacy_and_unknown_roots_are_never_overwritten_by_draft_saves() {
        for root in [br#"{"format":"lightcraft-companion","version":1,"photos":{}}"#.as_slice(), b"unknown root".as_slice()] {
            let files = MemStore::new();
            let catalog = MemStore::new();
            files.set(super::super::archive::STATE_FILE, root.to_vec());
            let mut s = open(files.clone(), catalog);
            let photo = s.catalog.photos().next().unwrap().id;
            s.set_companion_editor(photo, EditorState { draft: "pending draft".into(), ..Default::default() }).unwrap();
            assert!(s.persist().is_err());
            assert_eq!(files.get(super::super::archive::STATE_FILE).unwrap(), root);
            assert_eq!(s.companion_editor(photo).unwrap().draft, "pending draft");
            assert_eq!(s.companion_dirty_editors(), vec![photo]);
        }
    }
    #[test]
    fn newly_authored_known_draft_can_be_saved_after_its_old_payload_is_damaged() {
        let files = MemStore::new();
        let mut s = open(files.clone(), MemStore::new());
        let photo = s.catalog.photos().next().unwrap().id;
        let authored = EditorState { draft: "known authored draft".into(), ..Default::default() };
        s.set_companion_editor(photo, authored.clone()).unwrap();
        s.persist().unwrap();
        let payload = files.files.lock().unwrap().keys().find(|key| key.ends_with(".txt")).cloned().unwrap();
        files.set(&payload, b"damaged bytes".to_vec());
        assert!(s.companion_editor(photo).is_err());
        s.set_companion_editor(photo, authored).unwrap();
        s.persist().unwrap();
        assert_eq!(s.companion_editor(photo).unwrap().draft, "known authored draft");
    }
}
