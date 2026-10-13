//! Actual captured-reader lifetime, shared by physical storage namespace. A
//! cancelled worker continues to hold its pin until its HistoryReader drops.
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, OnceLock},
};
#[derive(Default)]
pub(crate) struct State {
    pub headers: BTreeMap<u64, PhotoHeader>,
    next: u64,
    released: BTreeSet<PhotoId>,
}
#[derive(Default)]
pub(crate) struct Pins {
    pub state: Mutex<State>,
    pub conservative: bool,
}
impl Pins {
    pub(crate) fn release_photo(&self, photo: PhotoId) {
        if !self.conservative {
            self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).released.insert(photo);
        }
    }
    pub(crate) fn has_releases(&self) -> bool {
        !self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).released.is_empty()
    }
    pub(crate) fn photo_released(&self, photo: PhotoId) -> bool {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).released.contains(&photo)
    }
    pub(crate) fn take_release(&self) -> Option<PhotoId> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).released.pop_first()
    }
}
pub(crate) fn for_store(store: &dyn Store) -> Arc<Pins> {
    let Some(namespace) = store.retention_namespace() else { return Arc::default() };
    // Keep only unconsumed release debt after a namespace's last owner drops;
    // reopening that namespace can resume maintenance without scanning photos.
    static REGISTRY: OnceLock<Mutex<BTreeMap<String, Arc<Pins>>>> = OnceLock::new();
    let mut registry = REGISTRY.get_or_init(Mutex::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    registry.retain(|_, p| Arc::strong_count(p) > 1 || p.has_releases());
    if let Some(pins) = registry.get(&namespace) {
        return pins.clone();
    }
    let pins = Arc::new(Pins { conservative: namespace == "native-windows-protected", state: Mutex::default() });
    registry.insert(namespace, pins.clone());
    pins
}
/// Opaque retention for a captured immutable graph; actual drop retires it.
pub struct HistoryRetention {
    pins: Arc<Pins>,
    id: u64,
    _library_lock: Option<Arc<lightcraft_catalog::LibraryLock>>,
}
impl Drop for HistoryRetention {
    fn drop(&mut self) {
        let mut state = self.pins.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(header) = state.headers.remove(&self.id)
            && !self.pins.conservative
        {
            // The Windows shared domain already keeps every native cleanup
            // pending while any reader exists; do not route foreign photo IDs.
            state.released.insert(header.photo_id);
        }
    }
}
pub(super) fn capture(
    pins: &Arc<Pins>,
    store: &mut dyn Store,
    header: &PhotoHeader,
    library_lock: Option<Arc<lightcraft_catalog::LibraryLock>>,
) -> Result<HistoryRetention> {
    let mut state = pins.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.headers.len() >= 64 || pagination::serialized_length(header)? > SMALL as usize || !valid_transaction(&header.transaction_id) {
        return Err(error("captured reader retention capacity exceeded"));
    }
    let frozen: Option<PhotoHeader> = read(store, &format!("{}/manifest-{}.json", base(header.photo_id), header.transaction_id), SMALL)?;
    if frozen.as_ref() != Some(header) {
        return Err(error("captured history graph no longer exists; reload its page"));
    }
    let id = state.next.checked_add(1).ok_or_else(|| error("reader retention sequence exhausted"))?;
    state.next = id;
    state.headers.insert(id, header.clone());
    Ok(HistoryRetention { pins: pins.clone(), id, _library_lock: library_lock })
}
