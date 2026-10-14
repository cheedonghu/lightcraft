//! Bounded page/fragment reads and immutable-prefix conversation updates.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryCursor {
    pub record: u64,
    pub offset: u64,
}
/// Ordered references into a slice's owned buffers; no text/body is duplicated.
#[derive(Clone, Copy, Debug)]
pub enum HistoryItem {
    Record { record: u64, index: usize },
    Fragment { record: u64, offset: u64, index: usize },
}
#[derive(Debug)]
pub struct HistorySlice {
    pub items: Vec<HistoryItem>,
    pub records: Vec<Value>,
    /// Oversize message text remains readable as consecutive decoded UTF-8 fragments.
    pub fragments: Vec<String>,
    pub next: Option<HistoryCursor>,
    pub bytes: usize,
}
pub(super) fn load_index(store: &mut dyn Store, photo: PhotoId, root: &IndexRoot, upper: u64) -> Result<Index> {
    validate_key(photo, &root.key, "pages/")?;
    let bytes = store.read_limited(&root.key, SMALL).map_err(error)?.ok_or_else(|| error("index missing"))?;
    if digest(&bytes) != root.digest {
        return Err(error("index digest mismatch"));
    }
    let index: Index = serde_json::from_slice(&bytes).map_err(error)?;
    if index.entries.is_empty()
        || index.entries.len() > PAGE_ITEMS
        || index.start.checked_add(index.entries.len() as u64) != Some(upper)
        || index.jumps.len() > 64
    {
        return Err(error("invalid index sequence"));
    }
    Ok(index)
}
pub(super) fn entry(store: &mut dyn Store, photo: PhotoId, root: Option<&IndexRoot>, count: u64, cursor: u64) -> Result<Entry> {
    if cursor >= count {
        return Err(error("record cursor outside history"));
    }
    let mut root = root.cloned();
    let mut upper = count;
    for _ in 0..128 {
        let current = root.ok_or_else(|| error("incomplete index"))?;
        let index = load_index(store, photo, &current, upper)?;
        if cursor >= index.start {
            let record = index.entries.get(usize::try_from(cursor - index.start).map_err(error)?).ok_or_else(|| error("record missing"))?.clone();
            validate_key(photo, &record.block, "blocks/")?;
            if record.length == 0
                || (record.length as u64 <= BLOCK && record.offset.checked_add(record.length as u64).is_none_or(|n| n > BLOCK))
                || (record.length as u64 > BLOCK && record.offset != 0)
            {
                return Err(error("invalid record bounds"));
            }
            let payload = record.chunk_payload.unwrap_or(BLOCK);
            if payload == 0 || payload > BLOCK || (record.chunk_payload.is_some() && payload != BLOCK - 32) {
                return Err(error("invalid chunk layout"));
            }
            return Ok(record);
        }
        if let Some(jump) = index.jumps.iter().rev().find(|j| j.upper > cursor && j.upper < index.start) {
            upper = jump.upper;
            root = Some(jump.root.clone());
        } else {
            upper = index.start;
            root = index.previous;
        }
    }
    Err(error("index seek exceeds bounded work"))
}
pub(super) fn range(store: &mut dyn Store, photo: PhotoId, entry: &Entry, offset: u64, length: usize) -> Result<Vec<u8>> {
    if length > 32 * 1024 * 1024 {
        return Err(error("record range exceeds bounded materialization; use fragments"));
    }
    let absolute = entry.offset.checked_add(offset).ok_or_else(|| error("record offset overflow"))?;
    let payload = entry.chunk_payload.unwrap_or(BLOCK);
    if payload == 0 || payload > BLOCK || (entry.chunk_payload.is_some() && payload != BLOCK - 32) {
        return Err(error("invalid chunk layout"));
    }
    let (stem, sequence) = entry.block.strip_suffix(".bin").and_then(|s| s.rsplit_once('-')).ok_or_else(|| error("invalid block"))?;
    let first = sequence.parse::<u64>().map_err(error)?;
    if entry.chunk_payload.is_none() {
        let sequence = first.checked_add(absolute / payload).ok_or_else(|| error("block sequence exhausted"))?;
        return read_blob(store, photo, &format!("{stem}-{sequence}.bin"), absolute % payload, length);
    }
    let mut output = Vec::new();
    let mut position = absolute;
    let mut remaining = length;
    while remaining > 0 {
        let sequence = first.checked_add(position / payload).ok_or_else(|| error("block sequence exhausted"))?;
        let key = format!("{stem}-{sequence}.bin");
        validate_key(photo, &key, "blocks/")?;
        let bytes = store.read_limited(&key, BLOCK).map_err(error)?.ok_or_else(|| error("record chunk missing"))?;
        let data_length = bytes.len().checked_sub(32).ok_or_else(|| error("record chunk has no digest"))?;
        let data = bytes.get(..data_length).ok_or_else(|| error("chunk bounds"))?;
        if bytes.get(data_length..) != Some(digest(data).as_bytes()) {
            return Err(error("record chunk digest mismatch"));
        }
        let offset = usize::try_from(position % payload).map_err(error)?;
        let take = remaining.min(data_length.saturating_sub(offset));
        if take == 0 {
            return Err(error("incomplete chunk"));
        }
        output.extend_from_slice(data.get(offset..offset + take).ok_or_else(|| error("chunk range invalid"))?);
        remaining -= take;
        position = position.checked_add(take as u64).ok_or_else(|| error("record offset exhausted"))?;
    }
    Ok(output)
}
fn slice(store: &mut dyn Store, h: &PhotoHeader, mut cursor: HistoryCursor, byte_budget: usize) -> Result<HistorySlice> {
    if !(1024..=256 * 1024).contains(&byte_budget) {
        return Err(error("history byte budget must be 1 KiB..256 KiB"));
    }
    let mut result = HistorySlice { items: Vec::new(), records: Vec::new(), fragments: Vec::new(), next: None, bytes: 0 };
    let mut scanned = 0usize;
    while cursor.record < h.message_count && scanned < 32 {
        scanned += 1;
        let entry = entry(store, h.photo_id, h.message_root.as_ref(), h.message_count, cursor.record)?;
        if cursor.offset >= entry.length as u64 {
            return Err(error("fragment offset outside record"));
        }
        let oversized = entry.length > byte_budget.saturating_sub(result.bytes) || cursor.offset > 0;
        if oversized && entry.chunk_payload.is_none() {
            if entry.length as u64 > BLOCK {
                return Err(error("unverified legacy chunk layout; original history retained"));
            }
            let complete = range(store, h.photo_id, &entry, 0, entry.length)?;
            if digest(&complete) != entry.digest {
                return Err(error("record digest mismatch"));
            }
        }
        let text_end = if oversized && let Some([start, end]) = entry.text_range {
            if start > end || end > entry.length as u64 {
                return Err(error("invalid message text range"));
            }
            cursor.offset = cursor.offset.max(start);
            if entry.text_range_version == Some(1) && cursor.offset >= end {
                if start == end {
                    result.items.push(HistoryItem::Fragment { record: cursor.record, offset: start, index: result.fragments.len() });
                    result.fragments.push(String::new());
                }
                cursor.record += 1;
                cursor.offset = 0;
                continue;
            }
            Some(if entry.text_range_version == Some(1) { end } else { entry.length as u64 })
        } else {
            None
        };
        let remaining = text_end.unwrap_or(entry.length as u64) - cursor.offset;
        let available = byte_budget.saturating_sub(result.bytes);
        if available < 1024 {
            break;
        }
        let length = usize::try_from(remaining.min(available as u64)).map_err(error)?;
        let mut bytes = range(store, h.photo_id, &entry, cursor.offset, length)?;
        // Old hints encoded a content heuristic, not the complete authored span.
        // Keep the known string start and discover its closing quote within this
        // one bounded slice. Never hydrate or rewrite the giant model record.
        let old_string_end = text_end.is_some() && entry.text_range_version != Some(1);
        let quote = old_string_end.then(|| string_end(&bytes)).flatten();
        if let Some(end) = quote {
            bytes.truncate(end);
        }
        if old_string_end && quote == Some(0) {
            result.items.push(HistoryItem::Fragment { record: cursor.record, offset: cursor.offset, index: result.fragments.len() });
            result.fragments.push(String::new());
            cursor.record += 1;
            cursor.offset = 0;
            continue;
        }
        if cursor.offset > 0 {
            let skip = bytes.iter().take(3).take_while(|b| **b & 0xc0 == 0x80).count();
            if skip > 0 {
                bytes.drain(..skip);
                cursor.offset = cursor.offset.checked_add(skip as u64).ok_or_else(|| error("UTF-8 cursor overflow"))?;
            }
        }
        if text_end.is_none() && cursor.offset == 0 && remaining == length as u64 {
            if digest(&bytes) != entry.digest {
                return Err(error("record digest mismatch"));
            }
            result.items.push(HistoryItem::Record { record: cursor.record, index: result.records.len() });
            result.records.push(scan::selected(&bytes)?);
        } else {
            // Keep boundaries on complete UTF-8 scalars; the subsequent fragment resumes
            // at the exact remaining byte. No content or message is discarded.
            if let Err(e) = std::str::from_utf8(&bytes) {
                if e.error_len().is_some() {
                    return Err(error("invalid UTF-8 history fragment"));
                }
                bytes.truncate(e.valid_up_to());
            }
            result.items.push(HistoryItem::Fragment { record: cursor.record, offset: cursor.offset, index: result.fragments.len() });
            if text_end.is_some() {
                let safe = complete_escape_prefix(&bytes);
                bytes.truncate(safe);
                let encoded = String::from_utf8(bytes.clone()).map_err(error)?;
                let decoded: String = serde_json::from_str(&format!("\"{encoded}\"")).map_err(error)?;
                result.fragments.push(decoded);
            } else {
                result.fragments.push(String::from_utf8(bytes.clone()).map_err(error)?);
            }
        }
        result.bytes += bytes.len();
        cursor.offset = cursor.offset.checked_add(bytes.len() as u64).ok_or_else(|| error("fragment cursor overflow"))?;
        if quote.is_some() || cursor.offset == text_end.unwrap_or(entry.length as u64) {
            cursor.record += 1;
            cursor.offset = 0;
        }
        if result.bytes >= byte_budget {
            break;
        }
    }
    result.next = (cursor.record < h.message_count).then_some(cursor);
    Ok(result)
}
pub(super) fn metadata_raw(store: &mut dyn Store, h: &PhotoHeader) -> Result<Value> {
    if h.metadata_length > SMALL {
        return Ok(json!({"schema_version":2,"checkpoint":null,"metadata_deferred":true}));
    }
    let bytes = range(store, h.photo_id, &metadata_entry(h)?, 0, usize::try_from(h.metadata_length).map_err(error)?)?;
    if digest(&bytes) != h.metadata_digest {
        return Err(error("metadata digest mismatch"));
    }
    serde_json::from_slice(&bytes).map_err(error)
}
pub(super) fn metadata(store: &mut dyn Store, h: &PhotoHeader) -> Result<Value> {
    if h.legacy_migrated {
        return Ok(
            json!({"schema_version":2,"checkpoint":null,"legacy_history":true,"legacy_history_complete":h.history_complete(),"metadata_retained":true}),
        );
    }
    let mut value = metadata_raw(store, h)?;
    if let Some(checkpoint) = value.get_mut("checkpoint").filter(|v| v.is_object()) {
        let revert: Option<Value> = read(store, &format!("{}/revert.json", base(h.photo_id)), SMALL)?;
        checkpoint["eligible"] = json!(revert.as_ref().is_some_and(|r| r["transaction_id"] == h.transaction_id && r["eligible"] == true));
    }
    Ok(value)
}
fn normalized_message(writer: &mut Writer<'_>, source: &Value) -> Result<Value> {
    let mut value = source.clone();
    if let Some(images) = value.get_mut("images").and_then(Value::as_array_mut) {
        for image in images {
            if let Some(encoded) = image.get("data").and_then(Value::as_str) {
                if encoded.len() > 176 * 1024 {
                    return Err(error("encoded preview exceeds bounds"));
                }
                let bytes = Base64::decode_vec(encoded).map_err(error)?;
                let (width, height) = validate_preview(&bytes)?;
                let key = writer.file("previews/", "jpg", &bytes)?;
                image.as_object_mut().ok_or_else(|| error("invalid preview"))?.remove("data");
                image["attachment"] = json!(key);
                image["digest"] = json!(digest(&bytes));
                image["length"] = json!(bytes.len());
                image["width"] = json!(width);
                image["height"] = json!(height);
            } else {
                read_preview_reference(writer.store, image)?;
            }
        }
    }
    Ok(value)
}
pub(super) fn tail(
    store: &mut dyn Store,
    photo: PhotoId,
    prefix: Option<&PhotoHeader>,
    messages: &[Value],
    rounds: &[Value],
    records: &[Value],
    metadata: &Value,
) -> Result<PendingCommit> {
    if let Some(prefix) = prefix {
        if prefix.photo_id != photo {
            return Err(error("prefix belongs to another photo"));
        }
        let stored: Option<PhotoHeader> = read(store, &format!("{}/manifest-{}.json", base(photo), prefix.transaction_id), SMALL)?;
        if stored.as_ref() != Some(prefix) {
            return Err(error("immutable prefix changed"));
        }
    }
    let epoch = header(store, photo)?.map(|h| h.conversation_epoch).unwrap_or(0);
    if prefix.is_some_and(|h| h.conversation_epoch != epoch) {
        return Err(error("conversation epoch changed"));
    }
    let fingerprint = recovery::fingerprint(&(prefix, messages, rounds, records, metadata))?;
    let (mut pending, complete) = recovery::prepare(store, photo, epoch, &fingerprint)?;
    if complete {
        return Ok(pending);
    }
    pending.reuse = prefix.cloned();
    pending.revert_override = None;
    write(store, &format!("{}/pending.json", base(photo)), &pending)?;
    let mut writer = Writer { store, pending, sequence: 0 };
    // Normalize one message at a time; retain neither the prior full history nor
    // an additional full tail copy. The writer packs bounded blocks/index pages.
    let mut root = prefix.and_then(|h| h.message_root.clone());
    let mut count = prefix.map(|h| h.message_count).unwrap_or(0);
    let mut batch = Vec::new();
    let mut batch_bytes = 0usize;
    for source in messages {
        let length = serialized_length(source)?;
        if !batch.is_empty() && (batch.len() == PAGE_ITEMS || batch_bytes.saturating_add(length) > BLOCK as usize) {
            root = writer.records_after(&batch, root, count)?;
            count = count.checked_add(batch.len() as u64).ok_or_else(|| error("message count exhausted"))?;
            batch.clear();
            batch_bytes = 0;
        }
        // A large caller-owned logical record streams directly; normalization is
        // only needed for its bounded preview array. Avoid a second record clone.
        if length as u64 > BLOCK {
            let images = source
                .get("images")
                .and_then(Value::as_array)
                .map(|images| {
                    let shell = json!({"images":images});
                    normalized_message(&mut writer, &shell).map(|v| v.get("images").cloned().unwrap_or(Value::Null))
                })
                .transpose()?;
            let record = BorrowedMessage { source, images };
            let length = serialized_length(&record)?;
            let mut entry = stream_record(&mut writer, &record, length)?;
            entry.text_range = text_range(record.source, record.images.as_ref())?;
            entry.text_range_version = Some(1);
            entry.preview_refs =
                record.images.as_ref().map(|images| preview_inventory(&json!({"images":images}))).unwrap_or_else(|| preview_inventory(record.source));
            let jumps = writer.jumps(root.as_ref(), count)?;
            let index = Index { start: count, entries: vec![entry], previous: root, jumps };
            let bytes = serde_json::to_vec(&index).map_err(error)?;
            root = Some(IndexRoot { key: writer.file("pages/", "json", &bytes)?, digest: digest(&bytes) });
            count = count.checked_add(1).ok_or_else(|| error("message count exhausted"))?;
        } else {
            let message = normalized_message(&mut writer, source)?;
            batch_bytes = batch_bytes.saturating_add(serialized_length(&message)?);
            batch.push(message);
        }
    }
    if !batch.is_empty() {
        root = writer.records_after(&batch, root, count)?;
        count = count.checked_add(batch.len() as u64).ok_or_else(|| error("message count exhausted"))?;
    }
    let round_count = prefix.map(|h| h.round_count).unwrap_or(0);
    let record_count = prefix.map(|h| h.record_count).unwrap_or(0);
    let round_root = writer.records_after(rounds, prefix.and_then(|h| h.round_root.clone()), round_count)?;
    // A late receipt may have patched an older immutable record after this runtime
    // tail began. Reuse the currently committed prefix of round records so the
    // next tail save cannot undo that truthful confirmation.
    let live = header(writer.store, photo)?;
    let record_prefix = if let Some(live) = live.filter(|h| h.conversation_epoch == epoch && h.record_count >= record_count) {
        truncate_records(&mut writer, live.record_root, live.record_count, record_count)?
    } else {
        prefix.and_then(|h| h.record_root.clone())
    };
    let record_root = writer.records_after(records, record_prefix, record_count)?;
    let length = serialized_length(metadata)?;
    let metadata_entry = if length as u64 > BLOCK {
        stream_record(&mut writer, metadata, length)?
    } else {
        let bytes = serde_json::to_vec(metadata).map_err(error)?;
        Entry {
            block: writer.blob(&bytes)?,
            offset: 0,
            length,
            digest: digest(&bytes),
            chunk_payload: None,
            text_range: None,
            text_range_version: None,
            preview_refs: Some(Vec::new()),
        }
    };
    let manifest = PhotoHeader {
        photo_id: photo,
        conversation_epoch: epoch,
        transaction_id: writer.pending.transaction_id.clone(),
        metadata: metadata_entry.block,
        message_root: root,
        round_root,
        message_count: count,
        round_count: round_count.checked_add(rounds.len() as u64).ok_or_else(|| error("round count exhausted"))?,
        record_root,
        record_count: record_count.checked_add(records.len() as u64).ok_or_else(|| error("record count exhausted"))?,
        metadata_digest: metadata_entry.digest,
        metadata_length: metadata_entry.length as u64,
        metadata_chunk_payload: metadata_entry.chunk_payload,
        record_patch: None,
        legacy_migrated: false,
        legacy_history_incomplete: prefix.is_some_and(|h| !h.history_complete()),
        legacy_round_count: prefix.map_or(0, |h| h.legacy_round_count),
        legacy_record_count: prefix.map_or(0, |h| h.legacy_record_count),
        legacy_message_count: prefix.map_or(0, PhotoHeader::original_message_count),
    };
    write(writer.store, &writer.pending.head.manifest, &manifest)?;
    writer.pending.head.manifest_digest = digest(&serde_json::to_vec(&manifest).map_err(error)?);
    write(writer.store, &format!("{}/pending.json", base(photo)), &writer.pending)?;
    verify_suffix(writer.store, &manifest, prefix)?;
    Ok(writer.pending)
}
impl Session {
    pub fn companion_history_slice(&mut self, photo: PhotoId, cursor: HistoryCursor, bytes: usize) -> Result<HistorySlice> {
        let h = self.companion_photo_header(photo)?.ok_or_else(|| error("conversation missing"))?;
        slice(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), &h, cursor, bytes)
    }
    pub fn companion_metadata(&mut self, photo: PhotoId) -> Result<Option<Value>> {
        let Some(h) = self.companion_photo_header(photo)? else { return Ok(None) };
        metadata(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), &h).map(Some)
    }
    /// Metadata from an already captured immutable graph, including verified
    /// read-only legacy graphs before root activation. No full source Value.
    pub fn companion_metadata_at(&mut self, header: &PhotoHeader) -> Result<Value> {
        metadata(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), header)
    }
    pub fn save_companion_tail(
        &mut self,
        photo: PhotoId,
        prefix: Option<&PhotoHeader>,
        messages: &[Value],
        rounds: &[Value],
        records: &[Value],
        metadata: &Value,
    ) -> Result<PhotoHeader> {
        if self.catalog.photo(photo).is_none() || self.companion_archive.legacy || self.companion_archive.blocked.is_some() {
            return Err(error("unavailable conversation"));
        }
        let result =
            tail(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), photo, prefix, messages, rounds, records, metadata)
                .and_then(|pending| self.companion_commit(&pending))
                .and_then(|_| self.companion_photo_header(photo)?.ok_or_else(|| error("committed header missing")));
        match result {
            Ok(header) => {
                self.capture_companion_revert_record(photo, header.conversation_epoch, prefix.map(|p| p.record_count).unwrap_or(0), records);
                self.companion_archive.tails.remove(&photo);
                if self.companion_archive.tails.is_empty()
                    && self.companion_archive.dirty_photos.is_empty()
                    && self.companion_archive.revert_dirty.is_empty()
                    && self.companion_archive.certificate_dirty.is_empty()
                    && self.companion_archive.receipts.is_empty()
                {
                    self.companion_archive.error = None;
                }
                Ok(header)
            }
            Err(error) => {
                self.companion_archive.tails.insert(
                    photo,
                    TailUpdate {
                        prefix: prefix.cloned(),
                        messages: messages.to_vec(),
                        rounds: rounds.to_vec(),
                        records: records.to_vec(),
                        metadata: metadata.clone(),
                    },
                );
                self.companion_archive.error = Some(error.to_string());
                Err(crate::EngineError::NotSaved(error.to_string()))
            }
        }
    }
}

struct Counter(usize);
impl std::io::Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.checked_add(bytes.len()).ok_or_else(|| std::io::Error::other("record length exhausted"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(super) fn serialized_length<T: Serialize + ?Sized>(value: &T) -> Result<usize> {
    let mut count = Counter(0);
    serde_json::to_writer(&mut count, value).map_err(error)?;
    Ok(count.0)
}
struct BlockSink<'a, 'b> {
    writer: &'a mut Writer<'b>,
    bytes: Vec<u8>,
    first: Option<String>,
    hash: lightcraft_preview::Hasher128,
}
impl std::io::Write for BlockSink<'_, '_> {
    fn write(&mut self, mut input: &[u8]) -> std::io::Result<usize> {
        let length = input.len();
        self.hash.update(input);
        while !input.is_empty() {
            let take = input.len().min(((BLOCK - 32) as usize).saturating_sub(self.bytes.len()));
            let (head, tail) = input.split_at(take);
            self.bytes.extend_from_slice(head);
            input = tail;
            if self.bytes.len() as u64 == BLOCK - 32 {
                self.flush()?;
            }
        }
        Ok(length)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if !self.bytes.is_empty() {
            self.bytes.extend_from_slice(digest(&self.bytes).as_bytes());
            let key = self.writer.file("blocks/", "bin", &self.bytes).map_err(std::io::Error::other)?;
            if self.first.is_none() {
                self.first = Some(key);
            }
            self.bytes.clear();
        }
        Ok(())
    }
}
pub(super) fn stream_record<T: Serialize + ?Sized>(writer: &mut Writer<'_>, value: &T, length: usize) -> Result<Entry> {
    use std::io::Write;
    let mut sink = BlockSink { writer, bytes: Vec::new(), first: None, hash: lightcraft_preview::Hasher128::new() };
    serde_json::to_writer(&mut sink, value).map_err(error)?;
    sink.flush().map_err(error)?;
    Ok(Entry {
        block: sink.first.ok_or_else(|| error("empty record"))?,
        offset: 0,
        length,
        digest: sink.hash.finish().to_string(),
        chunk_payload: Some(BLOCK - 32),
        text_range: None,
        text_range_version: None,
        preview_refs: None,
    })
}
struct RecordReader<'a> {
    store: &'a mut dyn Store,
    photo: PhotoId,
    entry: &'a Entry,
    offset: u64,
    hash: lightcraft_preview::Hasher128,
    cached: Vec<u8>,
    cached_start: u64,
}
impl std::io::Read for RecordReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let remaining = (self.entry.length as u64).saturating_sub(self.offset);
        if remaining == 0 {
            return Ok(0);
        }
        let length = usize::try_from(remaining.min(bytes.len().min(64 * 1024) as u64)).map_err(std::io::Error::other)?;
        if let Some(payload) = self.entry.chunk_payload {
            let start = self.offset / payload * payload;
            if self.cached.is_empty() || self.cached_start != start {
                let size = (self.entry.length as u64 - start).min(payload);
                self.cached = range(self.store, self.photo, self.entry, start, usize::try_from(size).map_err(std::io::Error::other)?)
                    .map_err(std::io::Error::other)?;
                self.cached_start = start;
            }
            let offset = usize::try_from(self.offset - self.cached_start).map_err(std::io::Error::other)?;
            let take = length.min(self.cached.len().saturating_sub(offset));
            let data = self.cached.get(offset..offset + take).ok_or_else(|| std::io::Error::other("cached chunk bounds"))?;
            bytes.get_mut(..take).ok_or_else(|| std::io::Error::other("reader bounds"))?.copy_from_slice(data);
            self.hash.update(data);
            self.offset += take as u64;
            Ok(take)
        } else {
            let data = range(self.store, self.photo, self.entry, self.offset, length).map_err(std::io::Error::other)?;
            let target = bytes.get_mut(..data.len()).ok_or_else(|| std::io::Error::other("reader bounds"))?;
            target.copy_from_slice(&data);
            self.hash.update(&data);
            self.offset += data.len() as u64;
            Ok(data.len())
        }
    }
}
pub(super) fn verify_record(store: &mut dyn Store, photo: PhotoId, entry: &Entry) -> Result<()> {
    // serde's IgnoredAny discards arbitrarily long strings/arrays without allocating
    // a whole Value. BufReader bounds range work and opened handles to one chunk.
    let reader = RecordReader { store, photo, entry, offset: 0, hash: lightcraft_preview::Hasher128::new(), cached: Vec::new(), cached_start: 0 };
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, reader);
    let mut decoder = serde_json::Deserializer::from_reader(&mut reader);
    serde::de::IgnoredAny::deserialize(&mut decoder).map_err(error)?;
    decoder.end().map_err(error)?;
    let reader = reader.into_inner();
    if reader.offset != entry.length as u64 || reader.hash.finish().to_string() != entry.digest {
        return Err(error("record digest mismatch"));
    }
    Ok(())
}

/// Only the frozen effect is materialized; unrelated diagnostic/message fields
/// are streamed and discarded. The complete logical record still verifies.
#[derive(Deserialize)]
pub(crate) struct RevertRecord {
    pub local_round_id: String,
    pub original: lightcraft_develop::DevelopSettings,
    pub recent_complete_result: lightcraft_develop::DevelopSettings,
}
pub(crate) fn revert_record(store: &mut dyn Store, header: &PhotoHeader, ordinal: u64) -> Result<RevertRecord> {
    let entry = entry(store, header.photo_id, header.record_root.as_ref(), header.record_count, ordinal)?;
    let reader = RecordReader {
        store,
        photo: header.photo_id,
        entry: &entry,
        offset: 0,
        hash: lightcraft_preview::Hasher128::new(),
        cached: Vec::new(),
        cached_start: 0,
    };
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, reader);
    let mut decoder = serde_json::Deserializer::from_reader(&mut reader);
    let record = RevertRecord::deserialize(&mut decoder).map_err(error)?;
    decoder.end().map_err(error)?;
    let reader = reader.into_inner();
    if reader.offset != entry.length as u64 || reader.hash.finish().to_string() != entry.digest {
        return Err(error("record digest mismatch"));
    }
    Ok(record)
}

/// Read-only worker view of a captured immutable graph. It owns at most one
/// native range handle at a time; each call has an explicit work/byte budget.
pub struct HistoryReader {
    _pin: reader_pins::HistoryRetention,
    store: Box<dyn Store>,
    header: PhotoHeader,
}
impl HistoryReader {
    /// Transfer verified graph/OS-lock retention into the completed page/view.
    /// The Store reader retires here; the graph retires when this guard drops.
    pub fn into_retention(self) -> reader_pins::HistoryRetention {
        self._pin
    }
    pub fn header(&self) -> &PhotoHeader {
        &self.header
    }
    pub fn messages(&mut self, cursor: u64, count: usize, max_bytes: usize) -> Result<RecordPage> {
        bounded_messages(self.store.as_mut(), &self.header, cursor, count, max_bytes)
    }
    /// Complete protocol fields from each captured record. Additive local data
    /// remains stored and never consumes the service input materialization budget.
    pub fn protocol_messages(&mut self, cursor: u64, count: usize, max_bytes: usize) -> Result<RecordPage> {
        protocol_messages(self.store.as_mut(), &self.header, cursor, count, max_bytes)
    }
    pub fn slice(&mut self, cursor: HistoryCursor, bytes: usize) -> Result<HistorySlice> {
        slice(self.store.as_mut(), &self.header, cursor, bytes)
    }
    /// A selected original reference, with bounded work buffers and no array quota.
    pub fn original_image_reference(&mut self, record: u64, index: u64) -> Result<(u64, Option<Value>)> {
        let entry = entry(self.store.as_mut(), self.header.photo_id, self.header.message_root.as_ref(), self.header.message_count, record)?;
        migration::image_reference(self.store.as_mut(), self.header.photo_id, &entry, index)
    }
    pub fn preview(&mut self, image: &Value) -> Result<Vec<u8>> {
        if image.get("data").is_some() || image.get("attachment").and_then(Value::as_str).is_some_and(|key| key.starts_with("companion/preview-")) {
            migration::preview(self.store.as_mut(), self.header.photo_id, image)
        } else {
            read_preview_reference(self.store.as_mut(), image)
        }
    }
}
fn protocol_messages(store: &mut dyn Store, header: &PhotoHeader, cursor: u64, count: usize, max_bytes: usize) -> Result<RecordPage> {
    if count == 0 || count > PAGE_ITEMS || max_bytes > 32 * 1024 * 1024 {
        return Err(error("invalid protocol read budget"));
    }
    let end = cursor.checked_add(count as u64).ok_or_else(|| error("protocol cursor overflow"))?.min(header.message_count);
    let mut records = Vec::new();
    let mut bytes = 0usize;
    for ordinal in cursor..end {
        let entry = entry(store, header.photo_id, header.message_root.as_ref(), header.message_count, ordinal)?;
        let value = migration::protocol_record(store, header.photo_id, &entry, max_bytes.saturating_sub(bytes))?;
        bytes = bytes.checked_add(serialized_length(&value)?).ok_or_else(|| error("protocol size overflow"))?;
        if bytes > max_bytes {
            return Err(error("complete history exceeds service read budget; local history retained"));
        }
        records.push(value);
    }
    Ok(RecordPage { records, next_cursor: (end < header.message_count).then_some(end) })
}
fn bounded_messages(store: &mut dyn Store, header: &PhotoHeader, cursor: u64, budget: usize, max_bytes: usize) -> Result<RecordPage> {
    if budget == 0 || budget > PAGE_ITEMS || max_bytes > 32 * 1024 * 1024 {
        return Err(error("invalid message read budget"));
    }
    let end = cursor.checked_add(budget as u64).ok_or_else(|| error("cursor overflow"))?.min(header.message_count);
    let mut bytes = 0usize;
    let mut records = Vec::new();
    for cursor in cursor..end {
        let entry = entry(store, header.photo_id, header.message_root.as_ref(), header.message_count, cursor)?;
        if entry.length > max_bytes.saturating_sub(bytes) {
            return Err(error("complete history exceeds service read budget; local history retained"));
        }
        let data = range(store, header.photo_id, &entry, 0, entry.length)?;
        if digest(&data) != entry.digest {
            return Err(error("record digest mismatch"));
        }
        records.push(serde_json::from_slice(&data).map_err(error)?);
        bytes += data.len();
    }
    Ok(RecordPage { records, next_cursor: (end < header.message_count).then_some(end) })
}
impl Session {
    pub fn companion_reader(&self, header: &PhotoHeader) -> Result<Option<HistoryReader>> {
        let library = self.library.as_ref().ok_or_else(|| error("library closed"))?;
        let Some(mut store) = library.files.background_writer() else { return Ok(None) };
        if library.files.retention_namespace().is_none() {
            return Err(error("background history reader requires stable Store retention namespace"));
        }
        let pin = reader_pins::capture(&self.companion_archive.readers, store.as_mut(), header, library.retain_read_lock())?;
        Ok(Some(HistoryReader { store, header: header.clone(), _pin: pin }))
    }
    /// Read-only captured selector on actual owned Fs/Mem handles, including
    /// protocol preparation and attachments. Main page reading retains its
    /// separate adapter policy; this never grants publication authority.
    pub fn companion_owned_history_reader(&self, header: &PhotoHeader) -> Result<Option<HistoryReader>> {
        let library = self.library.as_ref().ok_or_else(|| error("library closed"))?;
        let Some(mut store) = library.files.migration_writer() else { return Ok(None) };
        if library.files.retention_namespace().is_none() {
            return Err(error("attachment selector requires stable Store retention namespace"));
        }
        let pin = reader_pins::capture(&self.companion_archive.readers, store.as_mut(), header, library.retain_read_lock())?;
        Ok(Some(HistoryReader { store, header: header.clone(), _pin: pin }))
    }
    pub fn read_companion_historical_preview(&mut self, photo: PhotoId, image: &Value) -> Result<Vec<u8>> {
        if self.catalog.photo(photo).is_none() {
            return Err(error("no such photo"));
        }
        migration::preview(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), photo, image)
    }
    /// Complete verified protocol fields, excluding additive local metadata.
    pub fn companion_protocol_messages_bounded(&mut self, header: &PhotoHeader, cursor: u64, count: usize, bytes: usize) -> Result<RecordPage> {
        protocol_messages(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), header, cursor, count, bytes)
    }
    pub fn companion_messages_bounded(&mut self, header: &PhotoHeader, cursor: u64, count: usize, bytes: usize) -> Result<RecordPage> {
        bounded_messages(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), header, cursor, count, bytes)
    }
}

pub fn record_size(value: &Value) -> Result<usize> {
    serialized_length(value)
}
impl Session {
    pub fn companion_tail_unsaved(&self, photo: PhotoId) -> bool {
        self.companion_archive.tails.contains_key(&photo)
    }
    pub fn companion_history_slice_at(&mut self, header: &PhotoHeader, cursor: HistoryCursor, bytes: usize) -> Result<HistorySlice> {
        slice(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), header, cursor, bytes)
    }
}

#[derive(Clone)]
pub(crate) struct TailUpdate {
    pub prefix: Option<PhotoHeader>,
    pub messages: Vec<Value>,
    pub rounds: Vec<Value>,
    pub records: Vec<Value>,
    pub metadata: Value,
}
pub(crate) fn persist_tail(store: &mut dyn Store, photo: PhotoId, update: &TailUpdate) -> Result<String> {
    let pending = tail(store, photo, update.prefix.as_ref(), &update.messages, &update.rounds, &update.records, &update.metadata)?;
    commit(store, &pending).map(|()| pending.transaction_id)
}

struct BorrowedMessage<'a> {
    source: &'a Value,
    images: Option<Value>,
}
impl Serialize for BorrowedMessage<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let source = self.source.as_object().ok_or_else(|| serde::ser::Error::custom("message must be an object"))?;
        let mut map = serializer.serialize_map(Some(source.len()))?;
        for (key, value) in source {
            map.serialize_entry(key, if key == "images" { self.images.as_ref().unwrap_or(value) } else { value })?;
        }
        map.end()
    }
}

pub(super) fn metadata_entry(header: &PhotoHeader) -> Result<Entry> {
    Ok(Entry {
        block: header.metadata.clone(),
        offset: 0,
        length: usize::try_from(header.metadata_length).map_err(error)?,
        digest: header.metadata_digest.clone(),
        chunk_payload: header.metadata_chunk_payload,
        text_range: None,
        text_range_version: None,
        preview_refs: None,
    })
}

#[derive(Debug)]
pub struct RoundSearch {
    pub found: Option<Value>,
    pub next_cursor: Option<u64>,
}
impl Session {
    /// One bounded scan step. Callers advance the cursor off the frame; historical
    /// round count never becomes a local quota or one synchronous linear search.
    pub fn companion_round_search(&mut self, photo: PhotoId, local_id: &str, cursor: u64) -> Result<RoundSearch> {
        let h = self.companion_photo_header(photo)?.ok_or_else(|| error("conversation missing"))?;
        let (root, count) = if h.record_count > 0 { (h.record_root.as_ref(), h.record_count) } else { (h.round_root.as_ref(), h.round_count) };
        let page = page(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), photo, root, count, cursor, 32)?;
        let found = page.records.into_iter().find(|v| v["local_round_id"] == local_id);
        Ok(RoundSearch { next_cursor: if found.is_some() { None } else { page.next_cursor }, found })
    }
}

fn truncate_records(writer: &mut Writer<'_>, mut root: Option<IndexRoot>, mut count: u64, keep: u64) -> Result<Option<IndexRoot>> {
    if keep == 0 {
        return Ok(None);
    }
    if keep > count {
        return Err(error("record prefix no longer exists"));
    }
    for _ in 0..128 {
        let current = root.ok_or_else(|| error("record prefix missing"))?;
        let mut index = load_index(writer.store, writer.pending.photo, &current, count)?;
        if count == keep {
            return Ok(Some(current));
        }
        if index.start >= keep {
            count = index.start;
            root = index.previous;
            continue;
        }
        index.entries.truncate(usize::try_from(keep - index.start).map_err(error)?);
        index.jumps = writer.jumps(index.previous.as_ref(), index.start)?;
        let bytes = serde_json::to_vec(&index).map_err(error)?;
        return Ok(Some(IndexRoot { key: writer.file("pages/", "json", &bytes)?, digest: digest(&bytes) }));
    }
    Err(error("record prefix exceeds bounded tail distance"))
}
fn replace_recent(writer: &mut Writer<'_>, root: Option<IndexRoot>, count: u64, ordinal: u64, value: &Value) -> Result<IndexRoot> {
    let mut root = root;
    let mut upper = count;
    let mut newer = Vec::new();
    for _ in 0..128 {
        let current = root.ok_or_else(|| error("round record missing"))?;
        let mut index = load_index(writer.store, writer.pending.photo, &current, upper)?;
        if ordinal < index.start {
            upper = index.start;
            root = index.previous.clone();
            newer.push(index);
            continue;
        }
        let length = serialized_length(value)?;
        let replacement = if length as u64 > BLOCK {
            stream_record(writer, value, length)?
        } else {
            let bytes = serde_json::to_vec(value).map_err(error)?;
            Entry {
                block: writer.blob(&bytes)?,
                offset: 0,
                length,
                digest: digest(&bytes),
                chunk_payload: None,
                text_range: None,
                text_range_version: None,
                preview_refs: Some(Vec::new()),
            }
        };
        *index.entries.get_mut(usize::try_from(ordinal - index.start).map_err(error)?).ok_or_else(|| error("round record absent"))? = replacement;
        index.jumps = writer.jumps(index.previous.as_ref(), index.start)?;
        let bytes = serde_json::to_vec(&index).map_err(error)?;
        let mut current = IndexRoot { key: writer.file("pages/", "json", &bytes)?, digest: digest(&bytes) };
        for mut index in newer.into_iter().rev() {
            index.previous = Some(current);
            index.jumps = writer.jumps(index.previous.as_ref(), index.start)?;
            let bytes = serde_json::to_vec(&index).map_err(error)?;
            current = IndexRoot { key: writer.file("pages/", "json", &bytes)?, digest: digest(&bytes) };
        }
        return Ok(current);
    }
    Err(error("confirmation exceeds bounded recent-record range"))
}
impl Session {
    /// Patch an exact, already recorded execution receipt without loading messages
    /// or rewriting their prefix. Native transport ownership limits old receipts
    /// to the recent in-flight range; no photo parameters or certificate are changed.
    pub fn confirm_companion_execution(&mut self, identity: &super::super::RoundIdentity, plan: &str) -> Result<bool> {
        self.confirm_companion_receipt(identity, Some(plan))
    }
    pub fn confirm_companion_input(&mut self, identity: &super::super::RoundIdentity) -> Result<bool> {
        self.confirm_companion_receipt(identity, None)
    }
    fn confirm_companion_receipt(&mut self, identity: &super::super::RoundIdentity, plan: Option<&str>) -> Result<bool> {
        if identity.scope.library_generation != self.library_generation() {
            return Ok(false);
        }
        let photo = identity.scope.photo;
        let receipt = json!({"local_round_id":identity.local_round_id,"submission_seq":identity.submission_seq,"connection_generation":identity.connection_generation,"runtime_library_generation":identity.scope.library_generation,"plan":plan});
        let result = confirm_receipt(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), photo, &receipt);
        match &result {
            Ok(_) => {
                self.companion_archive.cleanup_photos.insert(photo);
                if let Ok(Some(p)) = self.companion_pending(photo)
                    && !p.head.manifest_digest.is_empty()
                {
                    self.companion_archive.cleanup_published.insert(photo, p.transaction_id);
                }
                if let Some(queued) = self.companion_archive.receipts.get_mut(&photo) {
                    queued.retain(|r| r != &receipt);
                    if queued.is_empty() {
                        self.companion_archive.receipts.remove(&photo);
                    }
                }
                if self.companion_archive.receipts.is_empty()
                    && self.companion_archive.tails.is_empty()
                    && self.companion_archive.dirty_photos.is_empty()
                    && self.companion_archive.revert_dirty.is_empty()
                {
                    self.companion_archive.error = None;
                }
            }
            Err(e) => {
                let queued = self.companion_archive.receipts.entry(photo).or_default();
                if !queued.contains(&receipt) {
                    queued.push(receipt);
                }
                self.companion_archive.error = Some(e.to_string());
            }
        }
        result
    }
}
/// A stored operation is replayed only into the archive. Runtime library identity
/// still gates external confirmations in Session, while recovery preserves the
/// original stored identity and cannot edit a photo or contact the backend.
pub(super) fn recover_receipt(store: &mut dyn Store, photo: PhotoId) -> Result<bool> {
    let p: Option<PendingCommit> = read(store, &format!("{}/pending.json", base(photo)), SMALL)?;
    if let Some(receipt) = p.and_then(|p| p.receipt) { confirm_receipt(store, photo, &receipt) } else { Ok(false) }
}
pub(crate) fn retry_receipt(store: &mut dyn Store, photo: PhotoId, receipt: &Value) -> Result<bool> {
    confirm_receipt(store, photo, receipt)
}
fn confirm_receipt(store: &mut dyn Store, photo: PhotoId, receipt: &Value) -> Result<bool> {
    if serialized_length(receipt)? > SMALL as usize {
        return Err(error("receipt operation exceeds bounded identity"));
    }
    let local_round_id =
        receipt["local_round_id"].as_str().filter(|id| valid_transaction(id)).ok_or_else(|| error("invalid stored receipt round"))?;
    let submission_seq = receipt["submission_seq"].as_u64().ok_or_else(|| error("invalid stored receipt sequence"))?;
    let connection_generation = receipt["connection_generation"].as_u64().ok_or_else(|| error("invalid stored receipt connection"))?;
    let library_generation = receipt["runtime_library_generation"].as_u64().ok_or_else(|| error("invalid stored receipt scope"))?;
    let plan = if receipt["plan"].is_null() { None } else { Some(receipt["plan"].as_str().ok_or_else(|| error("invalid stored receipt plan"))?) };
    let existing: Option<PendingCommit> = read(store, &format!("{}/pending.json", base(photo)), SMALL)?;
    let mut resumed = None;
    if let Some(pending) = existing {
        let visible = read(store, &format!("{}/HEAD.json", base(photo)), SMALL)?;
        recovery::validate(&pending, photo, &visible)?;
        if pending.revert_override.is_none() {
            return Err(error("uncommitted tail retains its reservation; retry receipt later"));
        }
        if pending.head.manifest_digest.is_empty() {
            if pending.receipt.as_ref() != Some(receipt) {
                return Err(error("another incomplete receipt retains its reservation"));
            }
            resumed = Some(pending);
        } else {
            commit(store, &pending)?;
            if let Err(e) = finish_pending(store, photo) {
                log::warn!("archive publication cleanup: {e}");
            }
        }
    }
    let Some(h) = header(store, photo)? else { return Ok(false) };
    let mut found = None;
    for ordinal in (h.record_count.saturating_sub(128)..h.record_count).rev() {
        let entry = entry(store, photo, h.record_root.as_ref(), h.record_count, ordinal)?;
        if entry.length > 32 * 1024 * 1024 {
            return Err(error("receipt record requires streaming update"));
        }
        let bytes = range(store, photo, &entry, 0, entry.length)?;
        if digest(&bytes) != entry.digest {
            return Err(error("record digest mismatch"));
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(error)?;
        if value["local_round_id"] == local_round_id
            && value["submission_seq"] == submission_seq
            && value["connection_generation"] == connection_generation
            && value["runtime_library_generation"] == library_generation
        {
            found = Some((ordinal, value));
            break;
        }
    }
    let Some((ordinal, mut value)) = found else { return Ok(false) };
    if let Some(plan) = plan {
        let Some(stage) = value.get_mut("stages").and_then(Value::as_array_mut).and_then(|s| s.iter_mut().find(|s| s["plan_id"] == plan)) else {
            return Ok(false);
        };
        let succeeded = stage["execution_outcome"] == "succeeded";
        let confirmed = stage["delivery_state"] == "confirmed";
        if confirmed && (!succeeded || value["backend_confirmed_plan"] == plan) {
            return Ok(true);
        }
        let Some(stage) = value.get_mut("stages").and_then(Value::as_array_mut).and_then(|s| s.iter_mut().find(|s| s["plan_id"] == plan)) else {
            return Ok(false);
        };
        stage["delivery_state"] = json!("confirmed");
        if succeeded {
            value["backend_confirmed_plan"] = json!(plan);
        }
    } else {
        if value["input_delivery_state"] == "confirmed" {
            return Ok(true);
        }
        value["input_delivery_state"] = json!("confirmed");
    }
    let old_head: Option<Head> = read(store, &format!("{}/HEAD.json", base(photo)), SMALL)?;
    let revert: Option<Value> = read(store, &format!("{}/revert.json", base(photo)), SMALL)?;
    let revert = revert.filter(|r| r["transaction_id"] == h.transaction_id).unwrap_or_else(|| json!({"eligible":false}));
    let tx = super::super::LocalRoundId::new().map_err(error)?.as_str().to_owned();
    let pending = resumed.unwrap_or(PendingCommit {
        photo,
        transaction_id: tx.clone(),
        expected_epoch: h.conversation_epoch,
        old_head,
        head: Head { manifest: format!("{}/manifest-{tx}.json", base(photo)), transaction_id: tx, manifest_digest: String::new() },
        reserved_files: 0,
        reuse: Some(h.clone()),
        revert_override: Some(revert),
        content_digest: None,
        receipt: Some(receipt.clone()),
    });
    write(store, &format!("{}/pending.json", base(photo)), &pending)?;
    let mut writer = Writer { store, pending, sequence: 0 };
    let root = replace_recent(&mut writer, h.record_root.clone(), h.record_count, ordinal, &value)?;
    let mut manifest = h;
    manifest.transaction_id = writer.pending.transaction_id.clone();
    manifest.record_root = Some(root);
    manifest.record_patch = Some(ordinal);
    write(writer.store, &writer.pending.head.manifest, &manifest)?;
    writer.pending.head.manifest_digest = digest(&serde_json::to_vec(&manifest).map_err(error)?);
    write(writer.store, &format!("{}/pending.json", base(photo)), &writer.pending)?;
    commit(writer.store, &writer.pending)?;
    if let Err(e) = finish_pending(writer.store, photo) {
        log::warn!("archive publication cleanup: {e}");
    }
    Ok(true)
}

impl Session {
    /// Runtime-only retention roots for bounded UI prefix caches. Retirement must
    /// preserve these immutable manifests and their graphs until release/close.
    pub fn pin_companion_prefix(&mut self, photo: PhotoId, header: Option<PhotoHeader>) -> Result<()> {
        if let Some(header) = header {
            if header.photo_id != photo
                || serialized_length(&header)? > SMALL as usize
                || (!self.companion_archive.prefixes.contains_key(&photo) && self.companion_archive.prefixes.len() >= 32)
            {
                return Err(error("runtime prefix retention capacity exceeded"));
            }
            let replaced = self.companion_archive.prefixes.get(&photo).is_some_and(|old| old != &header);
            self.companion_archive.prefixes.insert(photo, header);
            if replaced {
                self.companion_archive.readers.release_photo(photo);
                self.companion_archive.schedule_cleanup(photo);
            }
        } else if self.companion_archive.prefixes.remove(&photo).is_some() {
            self.companion_archive.readers.release_photo(photo);
            self.companion_archive.schedule_cleanup(photo);
        }
        Ok(())
    }
    pub fn companion_retained_headers(&self, photo: PhotoId) -> Vec<PhotoHeader> {
        self.companion_archive
            .prefixes
            .get(&photo)
            .into_iter()
            .cloned()
            .chain(self.companion_archive.tails.get(&photo).and_then(|t| t.prefix.as_ref()).cloned())
            .collect()
    }
}

pub(super) fn text_range(value: &Value, images: Option<&Value>) -> Result<Option<[u64; 2]>> {
    let Some(object) = value.as_object() else { return Ok(None) };
    let mut offset = 1u64;
    for (n, (key, field)) in object.iter().enumerate() {
        if n > 0 {
            offset = offset.checked_add(1).ok_or_else(|| error("text span overflow"))?;
        }
        offset = offset.checked_add(serialized_length(key)? as u64).and_then(|v| v.checked_add(1)).ok_or_else(|| error("text span overflow"))?;
        let field = if key == "images" { images.unwrap_or(field) } else { field };
        let length = serialized_length(field)? as u64;
        if key == "text" && field.is_string() {
            let start = offset.checked_add(1).ok_or_else(|| error("text span overflow"))?;
            // Match the normal message view: execution feedback and appended
            // machine photo-state JSON remain local records, not prose in the panel.
            let visible = if crate::companion::execution_feedback(value) { "" } else { crate::companion::authored_message_text(value) };
            let visible_length = serialized_length(visible)? as u64;
            let end = start.checked_add(visible_length.saturating_sub(2)).ok_or_else(|| error("text span overflow"))?;
            return Ok(Some([start, end]));
        }
        offset = offset.checked_add(length).ok_or_else(|| error("text span overflow"))?;
    }
    Ok(None)
}
fn complete_escape_prefix(bytes: &[u8]) -> usize {
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes.get(cursor) == Some(&b'\\') {
            let Some(next) = bytes.get(cursor + 1) else { return cursor };
            let length = if *next == b'u' { 6 } else { 2 };
            if cursor.saturating_add(length) > bytes.len() {
                return cursor;
            }
            cursor += length;
        } else {
            cursor += 1;
        }
    }
    cursor
}

mod projection;
mod scan;
pub use scan::{DisplayRecord, DisplayScan};
fn display_record(store: &mut dyn Store, photo: PhotoId, entry: &Entry) -> Result<Value> {
    if entry.length <= 256 * 1024 {
        let bytes = range(store, photo, entry, 0, entry.length)?;
        if digest(&bytes) != entry.digest {
            return Err(error("round digest mismatch"));
        }
        return scan::selected(&bytes);
    }
    let source = RecordReader { store, photo, entry, offset: 0, hash: lightcraft_preview::Hasher128::new(), cached: Vec::new(), cached_start: 0 };
    let (record, source) = projection::read(source)?;
    if source.offset != entry.length as u64 || source.hash.finish().to_string() != entry.digest {
        return Err(error("round digest mismatch"));
    }
    if serialized_length(&record)? > 1024 * 1024 {
        return Err(error("round display exceeds view budget; local history retained"));
    }
    Ok(record)
}
fn rounds_display(store: &mut dyn Store, header: &PhotoHeader, cursor: u64, count: usize) -> Result<RecordPage> {
    if count == 0 || count > 32 {
        return Err(error("round display page must contain 1..32 records"));
    }
    let (root, total) =
        if header.record_count > 0 { (header.record_root.as_ref(), header.record_count) } else { (header.round_root.as_ref(), header.round_count) };
    let end = cursor.saturating_add(count as u64).min(total);
    let mut records = Vec::new();
    let mut bytes = 0;
    let mut next = cursor;
    for index in cursor..end {
        let e = entry(store, header.photo_id, root, total, index)?;
        let record = display_record(store, header.photo_id, &e)?;
        let size = serialized_length(&record)?;
        if bytes + size > 1024 * 1024 && !records.is_empty() {
            break;
        }
        if size > 1024 * 1024 {
            return Err(error("round display exceeds view budget; local history retained"));
        }
        bytes += size;
        records.push(record);
        next = index + 1;
    }
    Ok(RecordPage { records, next_cursor: (next < total).then_some(next) })
}
impl HistoryReader {
    /// Captured graph, bounded page. Callers can scan off the frame and cancel between calls.
    pub fn rounds_display(&mut self, cursor: u64, count: usize) -> Result<RecordPage> {
        rounds_display(self.store.as_mut(), &self.header, cursor, count)
    }
}
impl Session {
    pub fn companion_rounds_display_at(&mut self, header: &PhotoHeader, cursor: u64, count: usize) -> Result<RecordPage> {
        rounds_display(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), header, cursor, count)
    }
}
fn message_display(store: &mut dyn Store, header: &PhotoHeader, index: u64) -> Result<Value> {
    let entry = entry(store, header.photo_id, header.message_root.as_ref(), header.message_count, index)?;
    let source = RecordReader {
        store,
        photo: header.photo_id,
        entry: &entry,
        offset: 0,
        hash: lightcraft_preview::Hasher128::new(),
        cached: Vec::new(),
        cached_start: 0,
    };
    let (mut owner, source) = projection::read(source)?;
    if source.offset != entry.length as u64 || source.hash.finish().to_string() != entry.digest {
        return Err(error("message owner digest mismatch"));
    }
    let id = owner["local_round_id"].as_str();
    if id.is_some_and(|id| id.is_empty() || id.len() > 128) {
        return Err(error("invalid local round identity"));
    }
    mark_old_hidden_text(&mut owner, &entry);
    Ok(owner)
}
fn message_round_id(store: &mut dyn Store, header: &PhotoHeader, index: u64) -> Result<Option<String>> {
    Ok(message_display(store, header, index)?["local_round_id"].as_str().map(str::to_owned))
}
impl HistoryReader {
    /// Bounded selected presentation fields, verified against the complete captured record digest.
    pub fn message_display(&mut self, index: u64) -> Result<Value> {
        message_display(self.store.as_mut(), &self.header, index)
    }
    pub fn message_round_id(&mut self, index: u64) -> Result<Option<String>> {
        message_round_id(self.store.as_mut(), &self.header, index)
    }
}
impl Session {
    pub fn companion_message_round_id_at(&mut self, header: &PhotoHeader, index: u64) -> Result<Option<String>> {
        message_round_id(self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut(), header, index)
    }
}

fn legacy_entry(store: &mut dyn Store, header: &PhotoHeader, index: u64) -> Result<Entry> {
    if header.legacy_round_count > header.round_count || header.legacy_record_count > header.record_count {
        return Err(error("legacy provenance exceeds graph"));
    }
    if index < header.legacy_round_count {
        entry(store, header.photo_id, header.round_root.as_ref(), header.round_count, index)
    } else {
        let index = index.checked_sub(header.legacy_round_count).ok_or_else(|| error("legacy cursor overflow"))?;
        if index >= header.legacy_record_count {
            return Err(error("legacy cursor outside original plans"));
        }
        entry(store, header.photo_id, header.record_root.as_ref(), header.record_count, index)
    }
}
impl HistoryReader {
    /// One original unassociated plan from the immutable retained legacy prefix.
    pub fn legacy_plan(&mut self, index: u64) -> Result<Value> {
        let entry = legacy_entry(self.store.as_mut(), &self.header, index)?;
        display_record(self.store.as_mut(), self.header.photo_id, &entry)
    }
}
fn string_end(bytes: &[u8]) -> Option<usize> {
    let mut offset = 0;
    while let Some(&byte) = bytes.get(offset) {
        if byte == b'"' {
            return Some(offset);
        }
        if byte == b'\\' {
            let length = if *bytes.get(offset + 1)? == b'u' { 6 } else { 2 };
            if offset + length > bytes.len() {
                return None;
            }
            offset += length;
        } else {
            offset += 1;
        }
    }
    None
}
fn mark_old_hidden_text(value: &mut Value, entry: &Entry) {
    if entry.text_range_version != Some(1)
        && entry.text_range.is_some_and(|[start, end]| start == end)
        && value["role"] == "user"
        && value.get("display_kind").is_none()
        && let Some(object) = value.as_object_mut()
    {
        object.insert("legacy_hidden_text".into(), Value::Bool(true));
    }
}
