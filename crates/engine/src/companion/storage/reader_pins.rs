//! Actual captured-reader lifetime, shared by physical storage namespace. A
//! cancelled worker continues to hold its pin until its HistoryReader drops.
use super::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, Weak},
};
#[derive(Default)]
pub(crate) struct State {
    pub headers: BTreeMap<u64, PhotoHeader>,
    next: u64,
}
#[derive(Default)]
pub(crate) struct Pins {
    pub state: Mutex<State>,
    pub conservative: bool,
}
pub(crate) fn for_store(store: &dyn Store) -> Arc<Pins> {
    let Some(namespace) = store.retention_namespace() else { return Arc::default() };
    static REGISTRY: OnceLock<Mutex<BTreeMap<String, Weak<Pins>>>> = OnceLock::new();
    let mut registry = REGISTRY.get_or_init(Mutex::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    registry.retain(|_, p| p.strong_count() > 0);
    if let Some(pins) = registry.get(&namespace).and_then(Weak::upgrade) {
        return pins;
    }
    let pins = Arc::new(Pins { conservative: namespace == "native-windows-protected", state: Mutex::default() });
    registry.insert(namespace, Arc::downgrade(&pins));
    pins
}
/// Opaque retention for a captured immutable graph; actual drop retires it.
pub struct HistoryRetention {
    pins: Arc<Pins>,
    id: u64,
}
impl Drop for HistoryRetention {
    fn drop(&mut self) {
        self.pins.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).headers.remove(&self.id);
    }
}
pub(super) fn capture(pins: &Arc<Pins>, store: &mut dyn Store, header: &PhotoHeader) -> Result<HistoryRetention> {
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
    Ok(HistoryRetention { pins: pins.clone(), id })
}
