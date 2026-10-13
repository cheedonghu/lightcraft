//! Format 2: immutable bounded records and indexes, with one visible photo HEAD.
//! No global photo index or growing per-photo JSON document is written.
use super::archive::{STATE_FILE, error};
use crate::{Result, Session};
use base64ct::{Base64, Encoding};
use lightcraft_catalog::{PhotoId, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const SMALL: u64 = 32 * 1024;
const BLOCK: u64 = 4 * 1024 * 1024;
const PAGE_ITEMS: usize = 128;
const FORMAT: &str = "lightcraft-companion";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhotoHeader {
    pub photo_id: PhotoId,
    pub conversation_epoch: u64,
    pub transaction_id: String,
    pub metadata: String,
    pub message_root: Option<IndexRoot>,
    pub round_root: Option<IndexRoot>,
    pub message_count: u64,
    pub round_count: u64,
    pub record_root: Option<IndexRoot>,
    pub record_count: u64,
    pub metadata_digest: String,
    pub metadata_length: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexRoot {
    pub key: String,
    pub digest: String,
}
#[derive(Clone, Debug)]
pub struct RecordPage {
    pub records: Vec<Value>,
    pub next_cursor: Option<u64>,
}
#[derive(Clone, Debug)]
pub struct CommitReceipt {
    pub photo: PhotoId,
    pub conversation_epoch: u64,
    pub transaction_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Head {
    manifest: String,
    transaction_id: String,
    manifest_digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    block: String,
    offset: u64,
    length: usize,
    digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Index {
    start: u64,
    entries: Vec<Entry>,
    previous: Option<IndexRoot>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingCommit {
    pub photo: PhotoId,
    pub transaction_id: String,
    pub expected_epoch: u64,
    old_head: Option<Head>,
    head: Head,
    /// Persisted before creating any file from this transaction's reserved range.
    pub reserved_files: u64,
}
fn base(photo: PhotoId) -> String {
    format!("companion/v2/photos/{}", photo.0)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:032x}", lightcraft_preview::hash_bytes(bytes).0)
}
fn read<T: serde::de::DeserializeOwned>(store: &mut dyn Store, key: &str, limit: u64) -> Result<Option<T>> {
    store.read_limited(key, limit).map_err(error)?.map(|b| serde_json::from_slice(&b).map_err(error)).transpose()
}
fn write<T: Serialize>(store: &mut dyn Store, key: &str, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(error)?;
    if bytes.len() as u64 > SMALL {
        return Err(error("manifest or index exceeds bound"));
    }
    store.write_atomic(key, &bytes).map_err(error)
}
fn validate_key(photo: PhotoId, key: &str, category: &str) -> Result<()> {
    let prefix = format!("{}/{category}", base(photo));
    let suffix = key.strip_prefix(&prefix).ok_or_else(|| error("cross-photo or invalid record reference"))?;
    if suffix.is_empty() || suffix.len() > 160 || !suffix.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') {
        return Err(error("invalid record path"));
    }
    Ok(())
}
fn valid_transaction(tx: &str) -> bool {
    tx.strip_prefix("local-").is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn header(store: &mut dyn Store, photo: PhotoId) -> Result<Option<PhotoHeader>> {
    let Some(head): Option<Head> = read(store, &format!("{}/HEAD.json", base(photo)), SMALL)? else { return Ok(None) };
    if !valid_transaction(&head.transaction_id) {
        return Err(error("invalid HEAD transaction"));
    }
    validate_key(photo, &head.manifest, "manifest-")?;
    if head.manifest != format!("{}/manifest-{}.json", base(photo), head.transaction_id) {
        return Err(error("manifest does not belong to HEAD transaction"));
    }
    let bytes = store.read_limited(&head.manifest, SMALL).map_err(error)?.ok_or_else(|| error("manifest missing"))?;
    if digest(&bytes) != head.manifest_digest {
        return Err(error("manifest digest mismatch"));
    }
    let h: PhotoHeader = serde_json::from_slice(&bytes).map_err(error)?;
    if h.photo_id != photo || h.transaction_id != head.transaction_id {
        return Err(error("manifest identity mismatch"));
    }
    Ok(Some(h))
}
fn page(store: &mut dyn Store, photo: PhotoId, root: Option<&IndexRoot>, count: u64, cursor: u64, budget: usize) -> Result<RecordPage> {
    if budget == 0 || budget > PAGE_ITEMS {
        return Err(error("page budget must be 1..128"));
    }
    let end = cursor.checked_add(budget as u64).ok_or_else(|| error("page cursor overflow"))?.min(count);
    if cursor >= count {
        return Ok(RecordPage { records: Vec::new(), next_cursor: None });
    }
    let mut key = root.cloned();
    let mut found = Vec::new();
    let mut upper = count;
    while let Some(current) = key {
        validate_key(photo, &current.key, "pages/")?;
        let bytes = store.read_limited(&current.key, SMALL).map_err(error)?.ok_or_else(|| error("index missing"))?;
        if digest(&bytes) != current.digest {
            return Err(error("index digest mismatch"));
        }
        let index: Index = serde_json::from_slice(&bytes).map_err(error)?;
        let index_end = index.start.checked_add(index.entries.len() as u64).ok_or_else(|| error("index overflow"))?;
        if index.entries.is_empty() || index.entries.len() > PAGE_ITEMS || index_end != upper {
            return Err(error("invalid index sequence"));
        }
        for (n, entry) in index.entries.iter().enumerate() {
            let seq = index.start + n as u64;
            if seq < cursor || seq >= end {
                continue;
            }
            validate_key(photo, &entry.block, "blocks/")?;
            if entry.length == 0
                || (entry.length as u64 <= BLOCK && entry.offset.checked_add(entry.length as u64).is_none_or(|n| n > BLOCK))
                || (entry.length as u64 > BLOCK && entry.offset != 0)
            {
                return Err(error("invalid record bounds"));
            }
            let bytes = read_blob(store, photo, &entry.block, entry.offset, entry.length)?;
            if bytes.len() != entry.length || digest(&bytes) != entry.digest {
                return Err(error("record digest mismatch"));
            }
            found.push((seq, serde_json::from_slice::<Value>(&bytes).map_err(error)?));
        }
        if index.start <= cursor {
            break;
        }
        upper = index.start;
        key = index.previous;
    }
    found.sort_by_key(|(seq, _)| *seq);
    if found.len() as u64 != end - cursor {
        return Err(error("incomplete index"));
    }
    Ok(RecordPage { records: found.into_iter().map(|(_, v)| v).collect(), next_cursor: (end < count).then_some(end) })
}
fn read_blob(store: &mut dyn Store, photo: PhotoId, first: &str, mut offset: u64, mut remaining: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut key = first.to_owned();
    while remaining > 0 {
        validate_key(photo, &key, "blocks/")?;
        if offset >= BLOCK {
            return Err(error("invalid block offset"));
        }
        let length = remaining.min((BLOCK - offset) as usize);
        let chunk = store.read_range(&key, offset, length).map_err(error)?;
        if chunk.len() != length {
            return Err(error("incomplete record block"));
        }
        bytes.extend_from_slice(&chunk);
        remaining -= length;
        if remaining > 0 {
            let (stem, seq) = key.strip_suffix(".bin").and_then(|s| s.rsplit_once('-')).ok_or_else(|| error("invalid continuation block"))?;
            let next = seq.parse::<u64>().map_err(error)?.checked_add(1).ok_or_else(|| error("block sequence overflow"))?;
            key = format!("{stem}-{next}.bin");
            offset = 0;
        }
    }
    Ok(bytes)
}
struct Writer<'a> {
    store: &'a mut dyn Store,
    pending: PendingCommit,
    sequence: u64,
}
impl Writer<'_> {
    fn file(&mut self, category: &str, suffix: &str, bytes: &[u8]) -> Result<String> {
        if bytes.len() as u64 > BLOCK || (category == "pages/" && bytes.len() as u64 > SMALL) {
            return Err(error("transaction file exceeds bound"));
        }
        self.sequence = self.sequence.checked_add(1).ok_or_else(|| error("transaction sequence exhausted"))?;
        self.pending.reserved_files = self.pending.reserved_files.max(self.sequence);
        write(self.store, &format!("{}/pending.json", base(self.pending.photo)), &self.pending)?;
        let key = format!("{}/{category}{}-{}.{suffix}", base(self.pending.photo), self.pending.transaction_id, self.sequence);
        self.store.write_atomic(&key, bytes).map_err(error)?;
        Ok(key)
    }
    fn blob(&mut self, bytes: &[u8]) -> Result<String> {
        let mut first = None;
        for chunk in bytes.chunks(BLOCK as usize) {
            let key = self.file("blocks/", "bin", chunk)?;
            if first.is_none() {
                first = Some(key);
            }
        }
        first.ok_or_else(|| error("empty block"))
    }
    fn records(&mut self, records: &[Value]) -> Result<Option<IndexRoot>> {
        let mut previous = None;
        let mut sequence = 0u64;
        let mut entries = Vec::new();
        let mut bytes = Vec::new();
        let mut offsets = Vec::new();
        for record in records {
            let data = serde_json::to_vec(record).map_err(error)?;
            if data.is_empty() {
                return Err(error("empty record"));
            }
            if data.len() as u64 > BLOCK {
                self.block(&mut bytes, &mut offsets, &mut entries)?;
                let first = self.blob(&data)?;
                entries.push(Entry { block: first, offset: 0, length: data.len(), digest: digest(&data) });
            } else {
                if bytes.len().saturating_add(data.len()) as u64 > BLOCK {
                    self.block(&mut bytes, &mut offsets, &mut entries)?;
                }
                offsets.push((bytes.len() as u64, data.len(), digest(&data)));
                bytes.extend_from_slice(&data);
            }
            if offsets.len() + entries.len() >= PAGE_ITEMS {
                self.block(&mut bytes, &mut offsets, &mut entries)?;
                let index = Index { start: sequence, entries: std::mem::take(&mut entries), previous };
                sequence = sequence.checked_add(index.entries.len() as u64).ok_or_else(|| error("record count overflow"))?;
                let bytes = serde_json::to_vec(&index).map_err(error)?;
                previous = Some(IndexRoot { key: self.file("pages/", "json", &bytes)?, digest: digest(&bytes) });
            }
        }
        self.block(&mut bytes, &mut offsets, &mut entries)?;
        if !entries.is_empty() {
            let index = Index { start: sequence, entries, previous };
            let bytes = serde_json::to_vec(&index).map_err(error)?;
            previous = Some(IndexRoot { key: self.file("pages/", "json", &bytes)?, digest: digest(&bytes) });
        }
        Ok(previous)
    }
    fn block(&mut self, bytes: &mut Vec<u8>, offsets: &mut Vec<(u64, usize, String)>, entries: &mut Vec<Entry>) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let key = self.file("blocks/", "bin", bytes)?;
        for (offset, length, digest) in offsets.drain(..) {
            entries.push(Entry { block: key.clone(), offset, length, digest });
        }
        bytes.clear();
        Ok(())
    }
}
pub(crate) fn stage(store: &mut dyn Store, photo: PhotoId, expected_epoch: u64, conversation: &Value) -> Result<PendingCommit> {
    let old = header(store, photo)?;
    if old.as_ref().map(|h| h.conversation_epoch).unwrap_or(0) != expected_epoch {
        return Err(error("conversation epoch changed"));
    }
    let pending_key = format!("{}/pending.json", base(photo));
    let existing = read::<PendingCommit>(store, &pending_key, SMALL)?;
    let actual: Option<Head> = read(store, &format!("{}/HEAD.json", base(photo)), SMALL)?;
    let reusable = existing.filter(|p| actual.as_ref().map(|h| &h.transaction_id) != Some(&p.transaction_id));
    let pending = if let Some(p) = reusable {
        if !valid_transaction(&p.transaction_id)
            || p.head.transaction_id != p.transaction_id
            || p.head.manifest != format!("{}/manifest-{}.json", base(photo), p.transaction_id)
            || p.photo != photo
            || p.expected_epoch != expected_epoch
            || p.old_head != actual
        {
            return Err(error("unresolved archive transaction; recovery required"));
        }
        p
    } else {
        let tx = super::LocalRoundId::new().map_err(error)?.as_str().to_owned();
        let head = Head { manifest: format!("{}/manifest-{tx}.json", base(photo)), transaction_id: tx.clone(), manifest_digest: String::new() };
        let old_head = actual;
        PendingCommit { photo, transaction_id: tx, expected_epoch, old_head, head, reserved_files: 0 }
    };
    write(store, &pending_key, &pending)?;
    let mut writer = Writer { store, pending, sequence: 0 };
    let mut normalized = conversation.clone();
    let messages = normalized.get_mut("messages").and_then(Value::as_array_mut).ok_or_else(|| error("messages missing"))?;
    for message in messages.iter_mut() {
        if let Some(images) = message.get_mut("images").and_then(Value::as_array_mut) {
            for image in images {
                if let Some(encoded) = image.get("data").and_then(Value::as_str) {
                    if encoded.len() > 176 * 1024 {
                        return Err(error("encoded preview exceeds bounds"));
                    }
                    let data = Base64::decode_vec(encoded).map_err(error)?;
                    let (width, height) = validate_preview(&data)?;
                    let name = writer.file("previews/", "jpg", &data)?;
                    image.as_object_mut().ok_or_else(|| error("invalid preview record"))?.remove("data");
                    image["attachment"] = json!(name);
                    image["digest"] = json!(digest(&data));
                    image["length"] = json!(data.len());
                    image["width"] = json!(width);
                    image["height"] = json!(height);
                }
            }
        }
    }
    let messages = normalized.get("messages").and_then(Value::as_array).ok_or_else(|| error("messages missing"))?;
    let rounds = normalized.get("rounds").and_then(Value::as_array).ok_or_else(|| error("rounds missing"))?;
    for message in messages {
        if let Some(images) = message.get("images").and_then(Value::as_array) {
            for image in images {
                let key = image.get("attachment").and_then(Value::as_str).ok_or_else(|| error("preview has no durable attachment"))?;
                validate_key(photo, key, "previews/")?;
                read_preview_reference(writer.store, image)?;
            }
        }
    }
    let message_root = writer.records(messages)?;
    let round_root = writer.records(rounds)?;
    let empty = Vec::new();
    let records = conversation.get("round_records").and_then(Value::as_array).unwrap_or(&empty);
    let record_root = writer.records(records)?;
    let mut metadata = normalized.clone();
    let object = metadata.as_object_mut().ok_or_else(|| error("conversation invalid"))?;
    object.remove("messages");
    object.remove("rounds");
    object.remove("round_records");
    let metadata_bytes = serde_json::to_vec(&metadata).map_err(error)?;
    let metadata_digest = digest(&metadata_bytes);
    let metadata_length = metadata_bytes.len() as u64;
    let metadata = writer.blob(&metadata_bytes)?;
    let manifest = PhotoHeader {
        photo_id: photo,
        conversation_epoch: expected_epoch,
        transaction_id: writer.pending.transaction_id.clone(),
        metadata,
        message_root,
        round_root,
        message_count: messages.len() as u64,
        round_count: rounds.len() as u64,
        record_root,
        record_count: records.len() as u64,
        metadata_digest,
        metadata_length,
    };
    write(writer.store, &writer.pending.head.manifest, &manifest)?;
    writer.pending.head.manifest_digest = digest(&serde_json::to_vec(&manifest).map_err(error)?);
    write(writer.store, &pending_key, &writer.pending)?;
    verify_graph(writer.store, &manifest)?;
    Ok(writer.pending)
}
fn verify_graph(store: &mut dyn Store, manifest: &PhotoHeader) -> Result<()> {
    validate_key(manifest.photo_id, &manifest.metadata, "blocks/")?;
    let actual = read_blob(store, manifest.photo_id, &manifest.metadata, 0, usize::try_from(manifest.metadata_length).map_err(error)?)?;
    if actual.len() as u64 != manifest.metadata_length || digest(&actual) != manifest.metadata_digest {
        return Err(error("metadata write verification failed"));
    }
    serde_json::from_slice::<Value>(&actual).map_err(error)?;
    for (root, count, messages) in [
        (manifest.message_root.as_ref(), manifest.message_count, true),
        (manifest.round_root.as_ref(), manifest.round_count, false),
        (manifest.record_root.as_ref(), manifest.record_count, false),
    ] {
        let mut cursor = 0;
        while cursor < count {
            let result = page(store, manifest.photo_id, root, count, cursor, PAGE_ITEMS)?;
            if messages {
                for message in &result.records {
                    if let Some(images) = message.get("images").and_then(Value::as_array) {
                        for image in images {
                            validate_key(
                                manifest.photo_id,
                                image.get("attachment").and_then(Value::as_str).ok_or_else(|| error("preview reference missing"))?,
                                "previews/",
                            )?;
                            read_preview_reference(store, image)?;
                        }
                    }
                }
            }
            cursor = result.next_cursor.unwrap_or(count);
        }
    }
    Ok(())
}
pub(crate) fn stored_epoch(store: &mut dyn Store, photo: PhotoId) -> Result<u64> {
    Ok(header(store, photo)?.map(|h| h.conversation_epoch).unwrap_or(0))
}
pub(crate) fn commit(store: &mut dyn Store, pending: &PendingCommit) -> Result<()> {
    if !valid_transaction(&pending.transaction_id)
        || pending.head.transaction_id != pending.transaction_id
        || pending.head.manifest != format!("{}/manifest-{}.json", base(pending.photo), pending.transaction_id)
    {
        return Err(error("invalid pending transaction"));
    }
    validate_key(pending.photo, &pending.head.manifest, "manifest-")?;
    let bytes = store.read_limited(&pending.head.manifest, SMALL).map_err(error)?.ok_or_else(|| error("staged manifest missing"))?;
    if digest(&bytes) != pending.head.manifest_digest {
        return Err(error("staged manifest digest mismatch"));
    }
    let staged: PhotoHeader = serde_json::from_slice(&bytes).map_err(error)?;
    if staged.photo_id != pending.photo || staged.transaction_id != pending.transaction_id || staged.conversation_epoch != pending.expected_epoch {
        return Err(error("staged identity mismatch"));
    }
    verify_graph(store, &staged)?;
    let head_key = format!("{}/HEAD.json", base(pending.photo));
    // Retrying the same transaction after an ambiguous fsync is idempotent.
    let visible: Option<Head> = read(store, &head_key, SMALL)?;
    if visible.as_ref() != Some(&pending.head) {
        if visible != pending.old_head {
            return Err(error("HEAD changed before commit"));
        }
        write(store, &head_key, &pending.head)?;
    }
    let manifest = header(store, pending.photo)?.ok_or_else(|| error("committed manifest missing"))?;
    let metadata: Value =
        serde_json::from_slice(&read_blob(store, pending.photo, &manifest.metadata, 0, usize::try_from(manifest.metadata_length).map_err(error)?)?)
            .map_err(error)?;
    write(
        store,
        &format!("{}/revert.json", base(pending.photo)),
        &json!({"transaction_id":pending.transaction_id,"local_round_id":metadata.pointer("/checkpoint/local_round_id"),"eligible":metadata.pointer("/checkpoint/eligible").and_then(Value::as_bool).unwrap_or(false)}),
    )?;
    // Cleanup cannot turn a visible valid commit into an archive-save error.
    if let Err(e) = store.remove(&format!("{}/pending.json", base(pending.photo))) {
        log::warn!("archive pending cleanup: {e}");
    }
    Ok(())
}
pub(crate) fn activate(store: &mut dyn Store) -> Result<()> {
    if let Some(root) = read::<Value>(store, STATE_FILE, SMALL)?
        && (root["format"] != FORMAT || root["version"] != 2 || root["layout"] != "photo-head-v2")
    {
        return Err(error("activation refuses an existing legacy or unknown root"));
    }
    write(store, STATE_FILE, &json!({"format":FORMAT,"version":2,"layout":"photo-head-v2"}))
}
impl Session {
    pub fn companion_photo_header(&mut self, photo: PhotoId) -> Result<Option<PhotoHeader>> {
        let lib = self.library.as_mut().ok_or_else(|| error("library closed"))?;
        header(lib.files.as_mut(), photo)
    }
    pub fn companion_messages_page(&mut self, photo: PhotoId, cursor: u64, budget: usize) -> Result<RecordPage> {
        let h = self.companion_photo_header(photo)?.ok_or_else(|| error("conversation missing"))?;
        page(
            self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(),
            photo,
            h.message_root.as_ref(),
            h.message_count,
            cursor,
            budget,
        )
    }
    pub fn companion_rounds_page(&mut self, photo: PhotoId, cursor: u64, budget: usize) -> Result<RecordPage> {
        let h = self.companion_photo_header(photo)?.ok_or_else(|| error("conversation missing"))?;
        let (root, count) = if h.record_count > 0 { (h.record_root.as_ref(), h.record_count) } else { (h.round_root.as_ref(), h.round_count) };
        page(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), photo, root, count, cursor, budget)
    }
    pub fn companion_round(&mut self, photo: PhotoId, local_round_id: &str) -> Result<Option<Value>> {
        let Some(h) = self.companion_photo_header(photo)? else { return Ok(None) };
        let store = self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut();
        let root = if h.record_count > 0 { h.record_root.as_ref() } else { h.round_root.as_ref() };
        let count = if h.record_count > 0 { h.record_count } else { h.round_count };
        let mut cursor = 0;
        while cursor < count {
            let p = page(store, photo, root, count, cursor, PAGE_ITEMS)?;
            if let Some(v) = p.records.into_iter().find(|v| v["local_round_id"] == local_round_id) {
                return Ok(Some(v));
            }
            cursor = p.next_cursor.unwrap_or(count);
        }
        Ok(None)
    }
    pub fn companion_pending(&mut self, photo: PhotoId) -> Result<Option<PendingCommit>> {
        let store = self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut();
        let pending = read::<PendingCommit>(store, &format!("{}/pending.json", base(photo)), SMALL)?;
        if pending.as_ref().is_some_and(|p| p.photo != photo || !valid_transaction(&p.transaction_id)) {
            return Err(error("invalid pending identity"));
        }
        Ok(pending)
    }
    pub fn companion_stage(&mut self, photo: PhotoId, expected_epoch: u64, conversation: &Value) -> Result<PendingCommit> {
        if self.catalog.photo(photo).is_none() {
            return Err(error("no such photo"));
        }
        if self.companion_archive.blocked.is_some() || self.companion_archive.legacy {
            return Err(error("legacy or unsupported archive; migration required"));
        }
        stage(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), photo, expected_epoch, conversation)
    }
    pub fn companion_commit(&mut self, pending: &PendingCommit) -> Result<CommitReceipt> {
        let store = self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut();
        commit(store, pending)?;
        if !self.companion_archive.v2 {
            activate(store)?;
            self.companion_archive.v2 = true;
        }
        Ok(CommitReceipt { photo: pending.photo, conversation_epoch: pending.expected_epoch, transaction_id: pending.transaction_id.clone() })
    }
    pub fn save_companion_conversation(&mut self, photo: PhotoId, conversation: Value) -> Result<()> {
        if let Some(e) = &self.companion_archive.blocked {
            return Err(error(e));
        }
        if self.catalog.photo(photo).is_none() {
            return Err(error("no such photo"));
        }
        if self.companion_archive.legacy {
            return self.set_photo_companion(photo, Some(conversation));
        }
        if self.library.is_none() {
            self.companion_archive.photos.insert(photo, conversation);
            return Ok(());
        }
        self.companion_archive.photos.insert(photo, conversation.clone());
        let result = self
            .companion_photo_header(photo)
            .and_then(|h| self.companion_stage(photo, h.map(|h| h.conversation_epoch).unwrap_or(0), &conversation))
            .and_then(|p| self.companion_commit(&p).map(|_| ()));
        match result {
            Ok(()) => {
                self.companion_archive.dirty_photos.remove(&photo);
                if self.companion_archive.dirty_photos.is_empty() && self.companion_archive.revert_dirty.is_empty() {
                    self.companion_archive.error = None;
                }
                Ok(())
            }
            Err(e) => {
                self.companion_archive.dirty_photos.insert(photo);
                self.companion_archive.error = Some(e.to_string());
                Err(crate::EngineError::NotSaved(e.to_string()))
            }
        }
    }
    pub fn load_companion_conversation(&mut self, photo: PhotoId) -> Result<Option<Value>> {
        if let Some(e) = &self.companion_archive.blocked {
            return Err(error(e));
        }
        if self.companion_archive.dirty_photos.contains(&photo) {
            return Ok(self.companion_archive.photos.get(&photo).cloned());
        }
        if !self.companion_archive.v2 {
            return Ok(self.photo_companion(photo)?.cloned());
        }
        let Some(h) = self.companion_photo_header(photo)? else { return Ok(None) };
        validate_key(photo, &h.metadata, "blocks/")?;
        let store = self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut();
        let bytes = read_blob(store, photo, &h.metadata, 0, usize::try_from(h.metadata_length).map_err(error)?)?;
        if bytes.len() as u64 != h.metadata_length || digest(&bytes) != h.metadata_digest {
            return Err(error("metadata digest mismatch"));
        }
        let mut value: Value = serde_json::from_slice(&bytes).map_err(error)?;
        let had_records = h.record_count > 0;
        for (name, root, count) in [
            ("messages", h.message_root.as_ref(), h.message_count),
            ("rounds", h.round_root.as_ref(), h.round_count),
            ("round_records", h.record_root.as_ref(), h.record_count),
        ] {
            let mut records = Vec::new();
            let mut cursor = 0;
            while cursor < count {
                let p = page(store, photo, root, count, cursor, PAGE_ITEMS)?;
                records.extend(p.records);
                cursor = p.next_cursor.unwrap_or(count);
            }
            value[name] = Value::Array(records);
        }
        if let Some(checkpoint) = value.get_mut("checkpoint").filter(|v| v.is_object()) {
            let revert: Option<Value> = read(store, &format!("{}/revert.json", base(photo)), SMALL)?;
            checkpoint["eligible"] = json!(revert.as_ref().is_some_and(|r| r["transaction_id"] == h.transaction_id && r["eligible"] == true));
        }
        if !had_records {
            value.as_object_mut().ok_or_else(|| error("invalid metadata"))?.remove("round_records");
        }
        self.companion_archive.photos.insert(photo, value.clone());
        Ok(Some(value))
    }
}

pub(super) fn read_preview(store: &mut dyn Store, key: &str) -> Result<Vec<u8>> {
    let (prefix, tail) = key.split_once("/previews/").ok_or_else(|| error("invalid preview path"))?;
    let photo = prefix.strip_prefix("companion/v2/photos/").and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| error("invalid preview owner"))?;
    validate_key(PhotoId(photo), key, "previews/")?;
    let expected = tail.strip_suffix(".jpg").ok_or_else(|| error("invalid preview extension"))?;
    let bytes = store.read_limited(key, 128 * 1024).map_err(error)?.ok_or_else(|| error("preview missing"))?;
    if expected.len() == 32 && expected.bytes().all(|b| b.is_ascii_hexdigit()) && digest(&bytes) != expected {
        return Err(error("preview digest mismatch"));
    }
    validate_preview(&bytes)?;
    Ok(bytes)
}
fn validate_preview(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.is_empty() || bytes.len() > 128 * 1024 {
        return Err(error("preview exceeds limits"));
    }
    let decoded = lightcraft_codecs::decode(bytes, lightcraft_codecs::DecodeOptions { max_size: None, max_pixels: 640 * 640 }).map_err(error)?;
    if decoded.format != lightcraft_codecs::Format::Jpeg || decoded.width.max(decoded.height) > 640 || decoded.orientation != 1 {
        return Err(error("preview must be an oriented JPEG at most 640 pixels"));
    }
    Ok((decoded.width, decoded.height))
}

pub(super) fn read_preview_reference(store: &mut dyn Store, image: &Value) -> Result<Vec<u8>> {
    if image.get("mime_type").and_then(Value::as_str) != Some("image/jpeg") {
        return Err(error("preview MIME does not match JPEG encoding"));
    }
    let key = image.get("attachment").and_then(Value::as_str).ok_or_else(|| error("preview reference missing"))?;
    let bytes = read_preview(store, key)?;
    let (width, height) = validate_preview(&bytes)?;
    if image.get("digest").and_then(Value::as_str) != Some(digest(&bytes).as_str())
        || image.get("length").and_then(Value::as_u64) != Some(bytes.len() as u64)
        || image.get("width").and_then(Value::as_u64) != Some(width as u64)
        || image.get("height").and_then(Value::as_u64) != Some(height as u64)
    {
        return Err(error("preview integrity mismatch"));
    }
    Ok(bytes)
}
impl crate::library::Library {
    pub fn read_companion_preview(&mut self, image: &Value) -> Result<Vec<u8>> {
        read_preview_reference(self.files.as_mut(), image)
    }
}

pub(crate) fn invalidate_revert(store: &mut dyn Store, photos: &std::collections::BTreeSet<PhotoId>) -> Result<()> {
    let mut failed = None;
    for photo in photos {
        let result = (|| {
            if let Some(h) = header(store, *photo)? {
                write(store, &format!("{}/revert.json", base(*photo)), &json!({"transaction_id":h.transaction_id,"eligible":false}))?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            failed = Some(e);
        }
    }
    failed.map_or(Ok(()), Err)
}
