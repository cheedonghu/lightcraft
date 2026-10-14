//! V1 span adapter into the existing pending/HEAD transaction module.
use super::super::migration::{Json, Span};
use super::*;
use lightcraft_catalog::SourceSnapshot;
use std::io::Write;
struct Sink<'a, 'b> {
    writer: &'a mut Writer<'b>,
    bytes: Vec<u8>,
    first: Option<String>,
    hash: lightcraft_preview::Hasher128,
    length: u64,
    chunked: bool,
}
impl Write for Sink<'_, '_> {
    fn write(&mut self, mut input: &[u8]) -> std::io::Result<usize> {
        let total = input.len();
        self.hash.update(input);
        self.length = self.length.checked_add(total as u64).ok_or_else(|| std::io::Error::other("legacy record length exhausted"))?;
        while !input.is_empty() {
            let take = input.len().min((self.payload() as usize).saturating_sub(self.bytes.len()));
            let (head, tail) = input.split_at(take);
            self.bytes.extend_from_slice(head);
            input = tail;
            if self.bytes.len() as u64 == self.payload() {
                self.flush()?;
            }
        }
        Ok(total)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if !self.bytes.is_empty() {
            if self.chunked {
                let hash = digest(&self.bytes);
                self.bytes.extend_from_slice(hash.as_bytes());
            }
            let key = self.writer.file("blocks/", "bin", &self.bytes).map_err(std::io::Error::other)?;
            if self.first.is_none() {
                self.first = Some(key)
            }
            self.bytes.clear();
        }
        Ok(())
    }
}
impl Sink<'_, '_> {
    fn payload(&self) -> u64 {
        if self.chunked { BLOCK - 32 } else { BLOCK }
    }
    fn finish(mut self) -> Result<Entry> {
        self.flush().map_err(error)?;
        Ok(Entry {
            block: self.first.ok_or_else(|| error("empty legacy record"))?,
            offset: 0,
            length: usize::try_from(self.length).map_err(error)?,
            digest: self.hash.finish().to_string(),
            chunk_payload: self.chunked.then_some(BLOCK - 32),
            text_range: None,
            text_range_version: None,
            preview_refs: None,
        })
    }
}
fn sink<'a, 'b>(writer: &'a mut Writer<'b>, chunked: bool) -> Sink<'a, 'b> {
    Sink { writer, bytes: Vec::new(), first: None, hash: lightcraft_preview::Hasher128::new(), length: 0, chunked }
}
fn raw(writer: &mut Writer<'_>, scanner: &mut Json<'_>, span: Span) -> Result<Entry> {
    let mut sink = sink(writer, span.len()? > BLOCK);
    scanner.copy(span, &mut sink)?;
    sink.finish()
}
fn append(writer: &mut Writer<'_>, entries: Vec<Entry>, previous: Option<IndexRoot>, count: u64) -> Result<IndexRoot> {
    let jumps = writer.jumps(previous.as_ref(), count)?;
    let index = Index { start: count, entries, previous, jumps };
    let bytes = serde_json::to_vec(&index).map_err(error)?;
    Ok(IndexRoot { key: writer.file("pages/", "json", &bytes)?, digest: digest(&bytes) })
}
fn records(writer: &mut Writer<'_>, source: &mut dyn SourceSnapshot, span: Span) -> Result<(Option<IndexRoot>, u64)> {
    let mut scanner = Json::new(source, span.start);
    scanner.expect(b'[')?;
    let mut root = None;
    let mut count = 0u64;
    let mut sequence = 0u64;
    let mut first = true;
    let mut bytes = Vec::new();
    let mut offsets = Vec::new();
    let mut entries = Vec::new();
    loop {
        scanner.space()?;
        if scanner.peek()? == Some(b']') {
            scanner.expect(b']')?;
            break;
        }
        if !first {
            scanner.expect(b',')?;
        }
        first = false;
        let span = scanner.value()?;
        let mut text = None;
        let mut fields = Json::new(scanner.source, span.start);
        fields.space()?;
        if fields.peek()? == Some(b'{') {
            fields.expect(b'{')?;
            let mut first = true;
            while let Some((_, name, value)) = fields.member(&mut first)? {
                if name.as_deref() == Some("text") {
                    let mut check = Json::new(fields.source, value.start);
                    if check.peek()? == Some(b'"') {
                        text = Some([value.start - span.start + 1, value.end - span.start - 1]);
                    }
                }
            }
        }
        // Preserve every source record verbatim. Additive fields must not pass
        // through serde's retained Value/depth limit during conversion.
        let length = usize::try_from(span.len()?).map_err(error)?;
        if length as u64 > BLOCK {
            writer.block(&mut bytes, &mut offsets, &mut entries)?;
            let mut entry = raw(writer, &mut scanner, span)?;
            entry.text_range = text;
            entries.push(entry);
        } else {
            if bytes.len().saturating_add(length) as u64 > BLOCK {
                writer.block(&mut bytes, &mut offsets, &mut entries)?;
            }
            let offset = bytes.len();
            scanner.copy(span, &mut bytes)?;
            let data = bytes.get(offset..).ok_or_else(|| error("legacy packed record bounds"))?;
            offsets.push((offset as u64, length, digest(data), text, None));
        }
        count = count.checked_add(1).ok_or_else(|| error("legacy record count exhausted"))?;
        if offsets.len().saturating_add(entries.len()) >= PACK_ITEMS {
            writer.block(&mut bytes, &mut offsets, &mut entries)?;
            let length = entries.len() as u64;
            root = Some(append(writer, std::mem::take(&mut entries), root, sequence)?);
            sequence = sequence.checked_add(length).ok_or_else(|| error("legacy page count exhausted"))?;
        }
    }
    writer.block(&mut bytes, &mut offsets, &mut entries)?;
    if !entries.is_empty() {
        root = Some(append(writer, entries, root, sequence)?);
    }
    Ok((root, count))
}
pub(crate) fn stage(store: &mut dyn Store, photo: PhotoId, source: &mut dyn SourceSnapshot, span: Span) -> Result<PhotoHeader> {
    let fingerprint = format!("legacy:{}:{}:{}:{}", source.identity().digest, photo.0, span.start, span.end);
    let (pending, complete) = recovery::prepare(store, photo, 0, &fingerprint)?;
    if complete {
        commit(store, &pending)?;
        return header(store, photo)?.ok_or_else(|| error("migrated header missing"));
    }
    write(store, &format!("{}/pending.json", base(photo)), &pending)?;
    let mut writer = Writer { store, pending, sequence: 0 };
    let (mut messages, mut rounds, mut record_root) = (None, None, None);
    let (mut message_count, mut round_count, mut record_count) = (0, 0, 0);
    let mut scanner = Json::new(source, span.start);
    scanner.space()?;
    let object = scanner.peek()? == Some(b'{');
    let mut history_incomplete = !object;
    let mut supported_schema = false;
    let mut messages_valid = false;
    if object {
        scanner.expect(b'{')?;
        let mut first = true;
        let mut seen = std::collections::BTreeSet::new();
        while let Some((_, name, value)) = scanner.member(&mut first)? {
            if let Some(name) = name.filter(|s| matches!(s.as_str(), "schema_version" | "messages" | "rounds" | "round_records")) {
                if !seen.insert(name.clone()) {
                    return Err(error("duplicate legacy record array; source protected"));
                }
                if name == "schema_version" {
                    supported_schema =
                        if value.len()? <= 8 { scanner.small(value)?.and_then(|v| v.as_u64()).is_some_and(|v| matches!(v, 1 | 2)) } else { false };
                    continue;
                }
                let mut check = Json::new(scanner.source, value.start);
                check.space()?;
                if check.peek()? != Some(b'[') {
                    history_incomplete = true;
                    continue;
                }
                if name == "messages" {
                    messages_valid = true;
                }
                let (root, count) = records(&mut writer, scanner.source, value)?;
                match name.as_str() {
                    "messages" => {
                        messages = root;
                        message_count = count
                    }
                    "rounds" => {
                        rounds = root;
                        round_count = count
                    }
                    _ => {
                        record_root = root;
                        record_count = count
                    }
                }
            }
        }
    }
    let mut scanner = Json::new(scanner.source, span.start);
    let mut out = sink(&mut writer, false);
    out.write_all(b"{").map_err(error)?;
    if object {
        scanner.expect(b'{')?;
        let mut first = true;
        let mut emitted = false;
        while let Some((key, name, value)) = scanner.member(&mut first)? {
            if name.as_deref().is_some_and(|s| matches!(s, "messages" | "rounds" | "round_records")) {
                continue;
            }
            if emitted {
                out.write_all(b",").map_err(error)?;
            }
            emitted = true;
            scanner.copy(key, &mut out)?;
            out.write_all(b":").map_err(error)?;
            scanner.copy(value, &mut out)?;
        }
    } else {
        out.write_all(b"\"legacy_payload\":").map_err(error)?;
        scanner.copy(span, &mut out)?;
    }
    out.write_all(b"}").map_err(error)?;
    let metadata = out.finish()?;
    let manifest = PhotoHeader {
        photo_id: photo,
        conversation_epoch: 0,
        transaction_id: writer.pending.transaction_id.clone(),
        metadata: metadata.block,
        message_root: messages,
        round_root: rounds,
        message_count,
        round_count,
        record_root,
        record_count,
        metadata_digest: metadata.digest,
        metadata_length: metadata.length as u64,
        metadata_chunk_payload: None,
        record_patch: None,
        legacy_migrated: true,
        legacy_history_incomplete: history_incomplete || !supported_schema || !messages_valid,
        legacy_round_count: round_count,
        legacy_record_count: record_count,
    };
    write(writer.store, &writer.pending.head.manifest, &manifest)?;
    writer.pending.head.manifest_digest = digest(&serde_json::to_vec(&manifest).map_err(error)?);
    write(writer.store, &format!("{}/pending.json", base(photo)), &writer.pending)?;
    verify_graph(writer.store, &manifest)?;
    commit(writer.store, &writer.pending)?;
    // The migration completion proof must become durable before09 removes the
    // pending identity; its caller confirms publication/cleanup after the proof.
    Ok(manifest)
}
pub(crate) fn verify(store: &mut dyn Store, expected: &PhotoHeader) -> Result<()> {
    let visible = header(store, expected.photo_id)?.ok_or_else(|| error("migration HEAD missing"))?;
    if visible != *expected || !visible.legacy_migrated {
        return Err(error("migration HEAD identity changed; source protected"));
    }
    verify_graph(store, expected)
}

/// Validate legacy immutable records with the same quota-free bounded parser,
/// then stream their complete independent record digest. No whole Value.
struct Record<'a> {
    store: &'a mut dyn Store,
    photo: PhotoId,
    entry: &'a Entry,
    identity: lightcraft_catalog::SourceIdentity,
    cache: Option<(u64, Vec<u8>)>,
}
impl SourceSnapshot for Record<'_> {
    fn identity(&self) -> &lightcraft_catalog::SourceIdentity {
        &self.identity
    }
    fn read_range(&mut self, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        if length > 256 * 1024 {
            return Err(std::io::Error::other("record source range exceeds bounded buffer"));
        }
        let Some(payload) = self.entry.chunk_payload.filter(|_| self.entry.offset == 0) else {
            return pagination::range(self.store, self.photo, self.entry, offset, length).map_err(std::io::Error::other);
        };
        if payload == 0 {
            return Err(std::io::Error::other("invalid record chunk payload"));
        }
        let end = offset
            .checked_add(length as u64)
            .filter(|end| *end <= self.entry.length as u64)
            .ok_or_else(|| std::io::Error::other("record source bounds"))?;
        let mut output = Vec::with_capacity(length);
        let mut at = offset;
        while at < end {
            let start = (at / payload).checked_mul(payload).ok_or_else(|| std::io::Error::other("record chunk offset overflow"))?;
            if self.cache.as_ref().is_none_or(|(cached, _)| *cached != start) {
                let remaining = (self.entry.length as u64).checked_sub(start).ok_or_else(|| std::io::Error::other("record chunk outside source"))?;
                let bytes = pagination::range(
                    self.store,
                    self.photo,
                    self.entry,
                    start,
                    usize::try_from(remaining.min(payload)).map_err(std::io::Error::other)?,
                )
                .map_err(std::io::Error::other)?;
                self.cache = Some((start, bytes));
            }
            let (_, bytes) = self.cache.as_ref().ok_or_else(|| std::io::Error::other("record chunk cache missing"))?;
            let local = usize::try_from(at - start).map_err(std::io::Error::other)?;
            let take = usize::try_from((end - at).min(payload - (at - start))).map_err(std::io::Error::other)?;
            let finish = local.checked_add(take).ok_or_else(|| std::io::Error::other("record cache range overflow"))?;
            output.extend_from_slice(bytes.get(local..finish).ok_or_else(|| std::io::Error::other("record chunk incomplete"))?);
            at = at.checked_add(take as u64).ok_or_else(|| std::io::Error::other("record position overflow"))?;
        }
        Ok(output)
    }
    fn verify_current(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn verify_record(store: &mut dyn Store, photo: PhotoId, entry: &Entry) -> Result<()> {
    let mut record = Record {
        store,
        photo,
        entry,
        identity: lightcraft_catalog::SourceIdentity { length: entry.length as u64, digest: entry.digest.clone(), token: "immutable-record".into() },
        cache: None,
    };
    let mut scanner = Json::new(&mut record, 0);
    scanner.value()?;
    scanner.space()?;
    if scanner.peek()?.is_some() {
        return Err(error("trailing legacy record data"));
    }
    let mut hash = lightcraft_preview::Hasher128::new();
    let mut offset = 0;
    while offset < entry.length as u64 {
        let count = (entry.length as u64 - offset).min(32 * 1024) as usize;
        let bytes = record.read_range(offset, count).map_err(error)?;
        hash.update(&bytes);
        offset += bytes.len() as u64;
    }
    if hash.finish().to_string() != entry.digest {
        return Err(error("legacy record digest mismatch"));
    }
    Ok(())
}

pub(in crate::companion) fn checked_header(store: &mut dyn Store, expected: &PhotoHeader) -> Result<PhotoHeader> {
    let visible = header(store, expected.photo_id)?.ok_or_else(|| error("legacy compatibility HEAD missing"))?;
    if visible != *expected || !visible.legacy_migrated {
        return Err(error("legacy compatibility HEAD changed"));
    }
    Ok(visible)
}

/// Legacy image references retain their original representation. Strict owner,
/// content hash, declared MIME and actual decoder bounds apply on every use.
pub(in crate::companion) fn preview(store: &mut dyn Store, photo: PhotoId, image: &Value) -> Result<Vec<u8>> {
    let mime = image.get("mime_type").and_then(Value::as_str).ok_or_else(|| error("legacy preview MIME missing"))?;
    let data = if let Some(encoded) = image.get("data") {
        if image.get("attachment").is_some() {
            return Err(error("ambiguous legacy preview sources"));
        }
        let encoded = encoded.as_str().filter(|text| text.len() <= 176 * 1024).ok_or_else(|| error("legacy preview exceeds bounds"))?;
        Base64::decode_vec(encoded).map_err(error)?
    } else {
        let key = image.get("attachment").and_then(Value::as_str).ok_or_else(|| error("legacy preview reference missing"))?;
        if key.starts_with("companion/v2/") {
            return read_preview_reference(store, image);
        }
        let prefix = format!("companion/preview-{}-", photo.0);
        let hash = key
            .strip_prefix(&prefix)
            .and_then(|key| key.strip_suffix(".bin"))
            .filter(|hash| hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| error("legacy preview owner/reference invalid"))?;
        let data = store.read_limited(key, 128 * 1024).map_err(error)?.ok_or_else(|| error("legacy preview missing"))?;
        if u128::from_str_radix(hash, 16).map_err(error)? != lightcraft_preview::hash_bytes(&data).0 {
            return Err(error("legacy preview digest mismatch"));
        }
        data
    };
    if image.get("length").and_then(Value::as_u64).is_some_and(|length| length != data.len() as u64) {
        return Err(error("legacy preview length mismatch"));
    }
    super::super::validate_history_image(mime, &data).map_err(error)?;
    Ok(data)
}

/// Select the complete known protocol fields, never retain additive local values.
/// Physical syntax and the entire independent record digest are verified first.
pub(super) fn protocol_record(store: &mut dyn Store, photo: PhotoId, entry: &Entry, max_bytes: usize) -> Result<Value> {
    verify_record(store, photo, entry)?;
    let mut source = Record {
        store,
        photo,
        entry,
        identity: lightcraft_catalog::SourceIdentity { length: entry.length as u64, digest: entry.digest.clone(), token: "immutable-record".into() },
        cache: None,
    };
    let mut scanner = Json::new(&mut source, 0);
    scanner.expect(b'{')?;
    let mut first = true;
    let mut fields = serde_json::Map::new();
    let mut retained = 0usize;
    while let Some((_, name, span)) = scanner.member(&mut first)? {
        if let Some(name) = name
            .filter(|name| matches!(name.as_str(), "role" | "text" | "tool_calls" | "tool_call_id" | "images" | "complete_state" | "display_kind"))
        {
            if fields.contains_key(&name) {
                return Err(error("ambiguous protocol field; local history retained"));
            }
            let value = if name == "images" {
                protocol_images(scanner.source, span, max_bytes.saturating_sub(retained))?
            } else {
                let length = usize::try_from(span.len()?).map_err(error)?;
                if length > max_bytes.saturating_sub(retained) {
                    return Err(error("complete history exceeds service read budget; local history retained"));
                }
                let mut bytes = Vec::new();
                scanner.copy(span, &mut bytes)?;
                serde_json::from_slice(&bytes).map_err(error)?
            };
            retained = retained.checked_add(pagination::serialized_length(&value)?).ok_or_else(|| error("protocol size overflow"))?;
            if retained > max_bytes {
                return Err(error("complete history exceeds service read budget; local history retained"));
            }
            fields.insert(name, value);
        }
    }
    Ok(Value::Object(fields))
}

/// Small display records already passed their immutable logical digest. Select
/// only known image fields after serde rejects additive nesting, without keeping
/// the additive descendants in a Value. The caller's record buffer is bounded.
pub(super) fn display_images(bytes: &[u8]) -> Result<Option<Value>> {
    let mut memory = lightcraft_catalog::MemStore::new();
    memory.set("record.json", bytes.to_vec());
    let mut source = memory.source_snapshot("record.json").map_err(error)?.ok_or_else(|| error("display record missing"))?;
    let mut scanner = Json::new(source.as_mut(), 0);
    scanner.expect(b'{')?;
    let mut first = true;
    let mut images = None;
    while let Some((_, name, span)) = scanner.member(&mut first)? {
        if name.as_deref() == Some("images") {
            if images.is_some() {
                return Err(error("ambiguous original image references"));
            }
            images = Some(protocol_images(scanner.source, span, bytes.len())?);
        }
    }
    Ok(images)
}

fn protocol_images(source: &mut dyn SourceSnapshot, span: Span, max_bytes: usize) -> Result<Value> {
    let mut scanner = Json::new(source, span.start);
    scanner.space()?;
    if scanner.peek()? == Some(b'n') {
        return Ok(Value::Null);
    }
    scanner.expect(b'[')?;
    let mut first = true;
    let mut values = Vec::new();
    let mut bytes = 2usize;
    loop {
        scanner.space()?;
        if scanner.peek()? == Some(b']') {
            scanner.expect(b']')?;
            break;
        }
        if !first {
            scanner.expect(b',')?;
        }
        first = false;
        let image = scanner.value()?;
        let value = selected_image(scanner.source, image)?;
        bytes = bytes.checked_add(pagination::serialized_length(&value)?.saturating_add(1)).ok_or_else(|| error("protocol image size overflow"))?;
        if bytes > max_bytes {
            return Err(error("complete history exceeds service read budget; local history retained"));
        }
        values.push(value);
    }
    Ok(Value::Array(values))
}

/// One original image reference plus a checked count, even when its message is
/// huge. The source record and every array element remain unmaterialized.
pub(super) fn image_reference(store: &mut dyn Store, photo: PhotoId, entry: &Entry, index: u64) -> Result<(u64, Option<Value>)> {
    verify_record(store, photo, entry)?;
    let mut source = Record {
        store,
        photo,
        entry,
        identity: lightcraft_catalog::SourceIdentity { length: entry.length as u64, digest: entry.digest.clone(), token: "immutable-record".into() },
        cache: None,
    };
    let mut scanner = Json::new(&mut source, 0);
    scanner.expect(b'{')?;
    let mut first = true;
    let mut images = None;
    while let Some((_, name, span)) = scanner.member(&mut first)? {
        if name.as_deref() == Some("images") {
            if images.is_some() {
                return Err(error("ambiguous original image references"));
            }
            images = Some(span);
        }
    }
    let Some(images) = images else { return Ok((0, None)) };
    let mut scanner = Json::new(scanner.source, images.start);
    scanner.space()?;
    if scanner.peek()? == Some(b'n') {
        return Ok((0, None));
    }
    scanner.expect(b'[')?;
    let mut total = 0u64;
    let mut selected = None;
    let mut first = true;
    loop {
        scanner.space()?;
        if scanner.peek()? == Some(b']') {
            scanner.expect(b']')?;
            break;
        }
        if !first {
            scanner.expect(b',')?;
        }
        first = false;
        let span = scanner.value()?;
        if total == index {
            selected = Some(selected_image(scanner.source, span)?);
        }
        total = total.checked_add(1).ok_or_else(|| error("original image count overflow"))?;
    }
    Ok((total, selected))
}

fn selected_image(source: &mut dyn SourceSnapshot, span: Span) -> Result<Value> {
    let mut image = Json::new(source, span.start);
    image.space()?;
    if image.peek()? != Some(b'{') {
        return Ok(Value::Null);
    }
    image.expect(b'{')?;
    let mut first = true;
    let mut fields = serde_json::Map::new();
    while let Some((_, name, value)) = image.member(&mut first)? {
        if let Some(name) =
            name.filter(|name| matches!(name.as_str(), "mime_type" | "data" | "attachment" | "length" | "digest" | "width" | "height"))
        {
            if fields.contains_key(&name) {
                return Err(error("ambiguous original image field"));
            }
            fields.insert(name, image.small(value)?.unwrap_or(Value::Null));
        }
    }
    Ok(Value::Object(fields))
}

/// Classify every original reference and durability-repeat valid attachments in
/// their original content-addressed location before the activation pointer.
/// Missing/corrupt references remain exact raw local history, never replacements.
pub(super) fn prove_references(store: &mut dyn Store, photo: PhotoId, entry: &Entry) -> Result<()> {
    let images = {
        let mut source = Record {
            store,
            photo,
            entry,
            identity: lightcraft_catalog::SourceIdentity {
                length: entry.length as u64,
                digest: entry.digest.clone(),
                token: "immutable-record".into(),
            },
            cache: None,
        };
        let mut scanner = Json::new(&mut source, 0);
        scanner.space()?;
        if scanner.peek()? != Some(b'{') {
            return Ok(());
        }
        scanner.expect(b'{')?;
        let mut first = true;
        let mut images = None;
        while let Some((_, name, span)) = scanner.member(&mut first)? {
            if name.as_deref() == Some("images") {
                if images.is_some() {
                    return Err(error("ambiguous original image references"));
                }
                images = Some(span);
            }
        }
        images
    };
    let Some(images) = images else { return Ok(()) };
    let mut cursor = images.start;
    let mut first = true;
    loop {
        let item = {
            let mut source = Record {
                store,
                photo,
                entry,
                identity: lightcraft_catalog::SourceIdentity {
                    length: entry.length as u64,
                    digest: entry.digest.clone(),
                    token: "immutable-record".into(),
                },
                cache: None,
            };
            let mut scanner = Json::new(&mut source, cursor);
            if cursor == images.start {
                scanner.space()?;
                if scanner.peek()? != Some(b'[') {
                    return Ok(());
                }
                scanner.expect(b'[')?;
            }
            scanner.space()?;
            if scanner.peek()? == Some(b']') {
                None
            } else {
                if !first {
                    scanner.expect(b',')?;
                }
                let span = scanner.value()?;
                let image = selected_image(scanner.source, span)?;
                Some((span.end, image))
            }
        };
        let Some((next, image)) = item else { break };
        cursor = next;
        first = false;
        if let Some(key) = image.get("attachment").and_then(Value::as_str) {
            let prefix = format!("companion/preview-{}-", photo.0);
            let legacy = key
                .strip_prefix(&prefix)
                .and_then(|key| key.strip_suffix(".bin"))
                .is_some_and(|hash| hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit()));
            if image.get("data").is_some() || (!legacy && validate_key(photo, key, "previews/").is_err()) {
                continue;
            }
            match store.read_limited(key, 128 * 1024) {
                Ok(None) => continue,
                Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData) => continue,
                Err(e) => return Err(error(e)),
                Ok(Some(_)) => {}
            }
        }
        if let Ok(bytes) = preview(store, photo, &image)
            && let Some(key) = image.get("attachment").and_then(Value::as_str)
        {
            store.write_atomic(key, &bytes).map_err(error)?;
        }
    }
    Ok(())
}
