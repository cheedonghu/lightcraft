//! Durable ownership and restartable bounded graph reclamation. No directory scan
//! or in-memory whole-history live set is required. A changed root snapshot
//! restarts marking before any further deletion.
use super::*;

#[derive(Serialize, Deserialize)]
struct Sealed<T> {
    format: String,
    version: u32,
    digest: String,
    data: T,
}
fn read_gc<T: serde::de::DeserializeOwned + Serialize>(store: &mut dyn Store, key: &str) -> Result<Option<T>> {
    let envelope: Option<Sealed<T>> = read(store, key, SMALL)?;
    envelope
        .map(|e| {
            if e.format != "lightcraft-companion-retirement" || e.version != 1 || e.digest != recovery::fingerprint(&e.data)? {
                return Err(error("unknown or damaged archive cleanup checkpoint; files protected"));
            }
            Ok(e.data)
        })
        .transpose()
}
fn write_gc<T: Serialize>(store: &mut dyn Store, key: &str, value: &T) -> Result<()> {
    write(store, key, &Sealed { format: "lightcraft-companion-retirement".into(), version: 1, digest: recovery::fingerprint(value)?, data: value })
}
#[derive(Debug, Default)]
pub struct CleanupReport {
    pub pending: bool,
    pub removed: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct OwnerRoot {
    transaction: String,
    ordinal: u64,
}
#[derive(Serialize, Deserialize)]
struct Owner {
    photo: PhotoId,
    transaction: String,
    ordinal: u64,
    previous: Option<OwnerRoot>,
    highwater: u64,
}
fn owner_key(photo: PhotoId, tx: &str) -> String {
    format!("{}/gc/owners/{tx}.json", base(photo))
}
fn owner_root_key(photo: PhotoId) -> String {
    format!("{}/gc/owners.json", base(photo))
}
fn check_root(root: &OwnerRoot) -> Result<()> {
    if !valid_transaction(&root.transaction) || root.ordinal == 0 {
        return Err(error("invalid allocation owner"));
    }
    Ok(())
}
fn load_owner(store: &mut dyn Store, photo: PhotoId, root: &OwnerRoot) -> Result<Owner> {
    check_root(root)?;
    let owner: Owner = read_gc(store, &owner_key(photo, &root.transaction))?.ok_or_else(|| error("allocation owner missing"))?;
    if owner.photo != photo
        || owner.transaction != root.transaction
        || owner.ordinal != root.ordinal
        || owner.previous.as_ref().is_some_and(|r| r.ordinal.checked_add(1) != Some(root.ordinal))
        || (owner.previous.is_none() && root.ordinal != 1)
    {
        return Err(error("invalid allocation ownership sequence"));
    }
    if let Some(previous) = owner.previous.as_ref() {
        check_root(previous)?;
    }
    Ok(owner)
}
/// This transfer must be durable before pending can be removed or replaced.
pub(super) fn own(store: &mut dyn Store, p: &PendingCommit) -> Result<()> {
    let key = owner_root_key(p.photo);
    let current: Option<OwnerRoot> = read_gc(store, &key)?;
    if let Some(root) = current.as_ref() {
        check_root(root)?;
    }
    let root = if current.as_ref().is_some_and(|r| r.transaction == p.transaction_id) {
        let root = current.ok_or_else(|| error("allocation owner missing"))?;
        let mut owner = load_owner(store, p.photo, &root)?;
        if owner.highwater > p.reserved_files {
            return Err(error("allocation reservation shrank"));
        }
        owner.highwater = p.reserved_files;
        // Repeat both bounded writes: visible ownership is not durable proof.
        write_gc(store, &owner_key(p.photo, &p.transaction_id), &owner)?;
        root
    } else {
        let ordinal = current.as_ref().map(|r| r.ordinal).unwrap_or(0).checked_add(1).ok_or_else(|| error("allocation owner sequence exhausted"))?;
        let root = OwnerRoot { transaction: p.transaction_id.clone(), ordinal };
        let owner = Owner { photo: p.photo, transaction: p.transaction_id.clone(), ordinal, previous: current, highwater: p.reserved_files };
        write_gc(store, &owner_key(p.photo, &p.transaction_id), &owner)?;
        root
    };
    write_gc(store, &key, &root)
}
#[derive(Clone, Serialize, Deserialize)]
struct Root {
    transaction: String,
    digest: String,
}
#[derive(Serialize, Deserialize)]
struct Work {
    photo: PhotoId,
    cycle: String,
    snapshot: String,
    roots: Vec<Root>,
    root: usize,
    kind: u8,
    metadata_chunk: u64,
    index: Option<IndexRoot>,
    upper: u64,
    entry: usize,
    block: u64,
    preview: usize,
    preserve_previews: bool,
    sweep: Option<OwnerRoot>,
    sequence: u64,
    category: u8,
    done: bool,
}
#[derive(Serialize, Deserialize)]
struct Mark {
    key: String,
    cycle: String,
}
fn mark_key(photo: PhotoId, key: &str) -> String {
    format!("{}/gc/marks/{}.json", base(photo), digest(key.as_bytes()))
}
fn mark(store: &mut dyn Store, photo: PhotoId, key: &str, cycle: &str) -> Result<()> {
    let marker = mark_key(photo, key);
    if let Some(old) = read_gc::<Mark>(store, &marker)?
        && old.key != key
    {
        return Err(error("allocation marker identity collision"));
    }
    // Repeat this bounded marker write after any ambiguous sync error.
    write_gc(store, &marker, &Mark { key: key.into(), cycle: cycle.into() })
}
fn chunk_key(photo: PhotoId, e: &Entry, chunk: u64) -> Result<Option<String>> {
    validate_key(photo, &e.block, "blocks/")?;
    let payload = e.chunk_payload.unwrap_or(BLOCK);
    if payload == 0 || payload > BLOCK || (e.chunk_payload.is_some() && payload != BLOCK - 32) || e.length == 0 {
        return Err(error("invalid retained record layout"));
    }
    let end = e.offset.checked_add(e.length as u64).ok_or_else(|| error("retained record overflow"))?;
    let chunks = end.checked_add(payload - 1).ok_or_else(|| error("retained record overflow"))? / payload;
    if chunk >= chunks {
        return Ok(None);
    }
    let (stem, sequence) = e.block.strip_suffix(".bin").and_then(|s| s.rsplit_once('-')).ok_or_else(|| error("invalid retained block"))?;
    let sequence = sequence.parse::<u64>().map_err(error)?.checked_add(chunk).ok_or_else(|| error("retained block sequence exhausted"))?;
    let key = format!("{stem}-{sequence}.bin");
    validate_key(photo, &key, "blocks/")?;
    Ok(Some(key))
}
fn captured(store: &mut dyn Store, photo: PhotoId, root: &Root) -> Result<PhotoHeader> {
    if !valid_transaction(&root.transaction) {
        return Err(error("invalid retained manifest"));
    }
    let key = format!("{}/manifest-{}.json", base(photo), root.transaction);
    let bytes = store.read_limited(&key, SMALL).map_err(error)?.ok_or_else(|| error("retained manifest missing"))?;
    if digest(&bytes) != root.digest {
        return Err(error("retained manifest changed"));
    }
    let h: PhotoHeader = serde_json::from_slice(&bytes).map_err(error)?;
    if h.photo_id != photo || h.transaction_id != root.transaction {
        return Err(error("retained manifest owner mismatch"));
    }
    Ok(h)
}
fn mark_step(store: &mut dyn Store, w: &mut Work) -> Result<()> {
    let root = w.roots.get(w.root).ok_or_else(|| error("retirement root missing"))?;
    let h = captured(store, w.photo, root)?;
    if w.kind == 0 {
        mark(store, w.photo, &format!("{}/manifest-{}.json", base(w.photo), h.transaction_id), &w.cycle)?;
        let entry = pagination::metadata_entry(&h)?;
        if let Some(key) = chunk_key(w.photo, &entry, w.metadata_chunk)? {
            mark(store, w.photo, &key, &w.cycle)?;
            w.metadata_chunk = w.metadata_chunk.checked_add(1).ok_or_else(|| error("metadata mark cursor exhausted"))?;
            return Ok(());
        }
        w.kind = 1;
    }
    if w.kind <= 3 {
        let (root, count) = match w.kind {
            1 => (h.message_root, h.message_count),
            2 => (h.round_root, h.round_count),
            _ => (h.record_root, h.record_count),
        };
        if w.index.is_none() && w.upper == 0 {
            w.index = root;
            w.upper = count;
        }
        if let Some(root) = w.index.clone() {
            let index = load_index(store, w.photo, &root, w.upper)?;
            mark(store, w.photo, &root.key, &w.cycle)?;
            if let Some(entry) = index.entries.get(w.entry) {
                if let Some(key) = chunk_key(w.photo, entry, w.block)? {
                    mark(store, w.photo, &key, &w.cycle)?;
                    w.block = w.block.checked_add(1).ok_or_else(|| error("block mark cursor exhausted"))?;
                    return Ok(());
                }
                // Older indexes have no complete preview inventory. Retain their
                // previews conservatively rather than parse unbounded JSON here.
                if w.kind == 1 {
                    if let Some(refs) = entry.preview_refs.as_ref() {
                        if refs.iter().map(String::len).sum::<usize>() > 256 {
                            return Err(error("invalid preview inventory bound"));
                        }
                        if let Some(key) = refs.get(w.preview) {
                            validate_key(w.photo, key, "previews/")?;
                            mark(store, w.photo, key, &w.cycle)?;
                            w.preview = w.preview.checked_add(1).ok_or_else(|| error("preview mark cursor exhausted"))?;
                            return Ok(());
                        }
                    } else {
                        w.preserve_previews = true;
                    }
                }
                w.entry = w.entry.checked_add(1).ok_or_else(|| error("record mark cursor exhausted"))?;
                w.block = 0;
                w.preview = 0;
                return Ok(());
            }
            w.index = index.previous;
            w.upper = index.start;
            w.entry = 0;
            if w.index.is_some() {
                return Ok(());
            }
            if w.upper != 0 {
                return Err(error("retained index incomplete"));
            }
        } else if count != 0 {
            return Err(error("retained index missing"));
        }
        w.kind += 1;
        w.upper = 0;
        w.entry = 0;
        w.block = 0;
        return Ok(());
    }
    w.root = w.root.checked_add(1).ok_or_else(|| error("root mark cursor exhausted"))?;
    w.kind = 0;
    w.metadata_chunk = 0;
    Ok(())
}
fn sweep_step(store: &mut dyn Store, w: &mut Work, pending_tx: Option<&str>) -> Result<usize> {
    let Some(root) = w.sweep.clone() else {
        w.done = true;
        return Ok(0);
    };
    let owner = load_owner(store, w.photo, &root)?;
    if pending_tx == Some(&owner.transaction) {
        w.sweep = owner.previous;
        w.sequence = 0;
        w.category = 0;
        return Ok(0);
    }
    let key = if w.sequence == 0 {
        format!("{}/manifest-{}.json", base(w.photo), owner.transaction)
    } else {
        let (category, suffix) = match w.category {
            0 => ("blocks", "bin"),
            1 => ("pages", "json"),
            2 => ("previews", "jpg"),
            _ => return Err(error("invalid cleanup category")),
        };
        format!("{}/{category}/{}-{}.{suffix}", base(w.photo), owner.transaction, w.sequence)
    };
    let marker = mark_key(w.photo, &key);
    let live: Option<Mark> = read_gc(store, &marker)?;
    if live.as_ref().is_some_and(|m| m.key != key) {
        return Err(error("cleanup marker identity mismatch"));
    }
    let mut removed = 0;
    if !live.as_ref().is_some_and(|m| m.cycle == w.cycle) && !(w.sequence > 0 && w.category == 2 && w.preserve_previews) {
        let exists = match store.read_range(&key, 0, 1) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(error(e)),
        };
        store.remove(&key).map_err(error)?;
        store.remove(&marker).map_err(error)?;
        removed = usize::from(exists);
    }
    if w.sequence == 0 {
        w.sequence = 1;
    } else if w.category == 2 {
        w.sequence = w.sequence.checked_add(1).ok_or_else(|| error("cleanup sequence exhausted"))?;
        w.category = 0;
    } else {
        w.category += 1;
    }
    if w.sequence > owner.highwater {
        w.sweep = owner.previous;
        w.sequence = 0;
        w.category = 0;
    }
    Ok(removed)
}
pub(crate) fn collect(store: &mut dyn Store, photo: PhotoId, mut retained: Vec<PhotoHeader>, budget: usize) -> Result<CleanupReport> {
    if !(1..=32).contains(&budget) {
        return Err(error("cleanup budget must be 1..32"));
    }
    if store.retention_namespace().is_none() {
        return Err(error("cleanup requires a stable Store namespace; files protected"));
    }
    let root: Option<Value> = read(store, STATE_FILE, SMALL)?;
    if !root.is_some_and(|r| r["format"] == FORMAT && r["version"] == 2 && r["layout"] == "photo-head-v2") {
        return Err(error("cleanup requires a known activated archive"));
    }
    let current = header(store, photo)?;
    let pending: Option<PendingCommit> = read(store, &format!("{}/pending.json", base(photo)), SMALL)?;
    if let Some(p) = pending.as_ref() {
        let visible = read(store, &format!("{}/HEAD.json", base(photo)), SMALL)?;
        recovery::validate(p, photo, &visible)?;
        // Before the staged manifest is finalized, arbitrary caller-owned preview
        // references may not yet have a graph. Protect all ownership until retry.
        if p.head.manifest_digest.is_empty() {
            return Ok(CleanupReport { pending: true, removed: 0 });
        }
        for head in p.old_head.iter().chain(std::iter::once(&p.head)) {
            let root = Root { transaction: head.transaction_id.clone(), digest: head.manifest_digest.clone() };
            retained.push(captured(store, photo, &root)?);
        }
        retained.extend(p.reuse.iter().cloned());
    }
    let owners: Option<OwnerRoot> = read_gc(store, &owner_root_key(photo))?;
    if let Some(owner) = owners.as_ref() {
        load_owner(store, photo, owner)?;
    }
    let mut all = std::collections::BTreeMap::new();
    for h in current.into_iter().chain(retained) {
        if h.photo_id != photo || !valid_transaction(&h.transaction_id) {
            return Err(error("invalid retained graph"));
        }
        let root = Root { transaction: h.transaction_id.clone(), digest: digest(&serde_json::to_vec(&h).map_err(error)?) };
        if let Some(previous) = all.insert(root.transaction.clone(), root.clone())
            && previous.digest != root.digest
        {
            return Err(error("conflicting retained manifest"));
        }
    }
    let roots: Vec<_> = all.into_values().collect();
    let snapshot = recovery::fingerprint(&(&roots, &owners, &pending))?;
    let work_key = format!("{}/gc/work.json", base(photo));
    let existing: Option<Work> = read_gc(store, &work_key)?;
    let mut work = if let Some(w) = existing.filter(|w| w.snapshot == snapshot) {
        if w.photo != photo || !valid_transaction(&w.cycle) || w.root > w.roots.len() || w.kind > 4 {
            return Err(error("invalid retirement cursor"));
        }
        w
    } else {
        Work {
            photo,
            cycle: super::super::LocalRoundId::new().map_err(error)?.as_str().into(),
            snapshot,
            roots,
            root: 0,
            kind: 0,
            metadata_chunk: 0,
            index: None,
            upper: 0,
            entry: 0,
            block: 0,
            preview: 0,
            preserve_previews: false,
            sweep: owners,
            sequence: 0,
            category: 0,
            done: false,
        }
    };
    let mut report = CleanupReport::default();
    for _ in 0..budget {
        if work.done {
            break;
        }
        if work.root < work.roots.len() {
            mark_step(store, &mut work)?;
        } else {
            report.removed += sweep_step(store, &mut work, pending.as_ref().map(|p| p.transaction_id.as_str()))?;
        }
        // A deletion may succeed before this write fails; repeating it is safe.
        write_gc(store, &work_key, &work)?;
    }
    if work.done {
        write_gc(store, &work_key, &work)?;
    }
    report.pending = !work.done;
    Ok(report)
}
impl Session {
    /// Reclamation never changes photo parameters or committed dialogue. Errors
    /// represent cleanup debt and can be retried independently of archive saving.
    pub fn collect_companion(&mut self, photo: PhotoId, budget: usize) -> Result<CleanupReport> {
        let mut retained = self.companion_retained_headers(photo);
        let pins = self.companion_archive.readers.clone();
        let readers = pins.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if pins.conservative && !readers.headers.is_empty() {
            return Ok(CleanupReport { pending: true, removed: 0 });
        }
        retained.extend(readers.headers.values().filter(|h| h.photo_id == photo).cloned());
        // Capture/drop and deletion share this bounded namespace critical section.
        collect(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), photo, retained, budget)
    }
}
