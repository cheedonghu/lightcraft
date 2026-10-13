//! Actual captured-reader lifetime, shared by physical storage namespace. A
//! cancelled worker continues to hold its pin until its HistoryReader drops.
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};
#[derive(Default)]
pub(crate) struct State {
    pub headers: BTreeMap<u64, PhotoHeader>,
    next: u64,
    released: BTreeSet<PhotoId>,
    retired: VecDeque<ArchiveDebt>,
}
pub(crate) struct ArchiveDebt {
    pub prefixes: BTreeMap<PhotoId, PhotoHeader>,
    pub active: BTreeSet<PhotoId>,
    pub waiting: BTreeSet<PhotoId>,
}
impl ArchiveDebt {
    fn empty(&self) -> bool {
        self.prefixes.is_empty() && self.active.is_empty() && self.waiting.is_empty()
    }
    fn take(&mut self) -> Option<PhotoId> {
        self.active.pop_first().or_else(|| self.waiting.pop_first()).or_else(|| self.prefixes.pop_first().map(|(photo, _)| photo))
    }
    fn contains(&self, photo: PhotoId) -> bool {
        self.active.contains(&photo) || self.waiting.contains(&photo) || self.prefixes.contains_key(&photo)
    }
}
#[derive(Default)]
pub(crate) struct Pins {
    pub state: Mutex<State>,
    protection: Option<Arc<Mutex<usize>>>,
}
impl Pins {
    // Lock order is always global physical pause, then owning namespace state.
    // A guard is held across capture and deletion; foreign headers never enter.
    pub(crate) fn physical_pause(&self) -> Option<MutexGuard<'_, usize>> {
        self.protection.as_ref().map(|p| p.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
    pub(crate) fn retire_archive(&self, debt: ArchiveDebt) {
        if !debt.empty() {
            self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retired.push_back(debt);
        }
    }
    pub(crate) fn release_photo(&self, photo: PhotoId) {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).released.insert(photo);
    }
    pub(crate) fn has_releases(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        !state.released.is_empty() || !state.retired.is_empty()
    }
    pub(crate) fn photo_released(&self, photo: PhotoId) -> bool {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.released.contains(&photo) || state.retired.iter().any(|debt| debt.contains(photo))
    }
    pub(crate) fn take_release(&self) -> Option<PhotoId> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(photo) = state.released.pop_first() {
            return Some(photo);
        }
        let debt = state.retired.front_mut()?;
        let photo = debt.take();
        if debt.empty() {
            state.retired.pop_front();
        }
        photo
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
    static PHYSICAL_PAUSE: OnceLock<Arc<Mutex<usize>>> = OnceLock::new();
    let protection = store.conservative_retention().then(|| PHYSICAL_PAUSE.get_or_init(|| Arc::new(Mutex::new(0))).clone());
    let pins = Arc::new(Pins { protection, state: Mutex::default() });
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
        let mut protection = self.pins.physical_pause();
        let mut state = self.pins.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(header) = state.headers.remove(&self.id) {
            state.released.insert(header.photo_id);
            if let Some(count) = protection.as_mut() {
                **count -= 1;
            }
        }
    }
}
pub(super) fn capture(
    pins: &Arc<Pins>,
    store: &mut dyn Store,
    header: &PhotoHeader,
    library_lock: Option<Arc<lightcraft_catalog::LibraryLock>>,
) -> Result<HistoryRetention> {
    let mut protection = pins.physical_pause();
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
    if let Some(count) = protection.as_mut() {
        **count += 1;
    }
    Ok(HistoryRetention { pins: pins.clone(), id, _library_lock: library_lock })
}
