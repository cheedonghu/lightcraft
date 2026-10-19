//! Resumable selected display projection for stores without a worker handle.
//! Each step advances <=256 KiB; an ignored value never becomes a retained buffer.
use super::*;

#[derive(Clone, Copy)]
pub enum DisplayRecord {
    Message(u64),
    Round(u64),
    LegacyPlan(u64),
}
pub struct DisplayScan {
    header: PhotoHeader,
    target: DisplayRecord,
    entry: Option<Entry>,
    offset: u64,
    hash: lightcraft_preview::Hasher128,
    small: Vec<u8>,
    fields: Fields,
}
impl DisplayScan {
    pub fn new(header: PhotoHeader, target: DisplayRecord) -> Self {
        Self { header, target, entry: None, offset: 0, hash: lightcraft_preview::Hasher128::new(), small: Vec::new(), fields: Fields::default() }
    }
    pub fn target(&self) -> DisplayRecord {
        self.target
    }
}
impl Session {
    /// One bounded continuation step against the scan's immutable captured graph.
    /// Returns a verified record only after its entire digest has been checked.
    pub fn companion_display_step(&mut self, scan: &mut DisplayScan) -> Result<Option<Value>> {
        let store = self.library.as_mut().ok_or_else(|| error("library closed"))?.files.as_mut();
        if scan.entry.is_none() {
            let h = &scan.header;
            if let DisplayRecord::LegacyPlan(index) = scan.target {
                scan.entry = Some(legacy_entry(store, h, index)?);
            } else {
                let (root, total, index) = match scan.target {
                    DisplayRecord::LegacyPlan(_) => return Err(error("legacy display cursor was not selected")),
                    DisplayRecord::Message(index) => (h.message_root.as_ref(), h.message_count, index),
                    DisplayRecord::Round(index) => {
                        if h.record_count > 0 {
                            (h.record_root.as_ref(), h.record_count, index)
                        } else {
                            (h.round_root.as_ref(), h.round_count, index)
                        }
                    }
                };
                scan.entry = Some(entry(store, h.photo_id, root, total, index)?);
            }
        }
        let e = scan.entry.as_ref().ok_or_else(|| error("display entry missing"))?;
        // Never straddle a chunk: physical verification reads at most one 4 MiB
        // block, plus the existing bounded index seek on the first step.
        let payload = e.chunk_payload.unwrap_or(BLOCK);
        let absolute = e.offset.checked_add(scan.offset).ok_or_else(|| error("display offset overflow"))?;
        let remaining = (e.length as u64).checked_sub(scan.offset).ok_or_else(|| error("display scan finished"))?;
        let length = remaining.min(256 * 1024).min(payload - absolute % payload) as usize;
        let bytes = range(store, scan.header.photo_id, e, scan.offset, length)?;
        scan.hash.update(&bytes);
        scan.offset += bytes.len() as u64;
        if e.length <= 256 * 1024 {
            scan.small.extend_from_slice(&bytes);
        } else {
            scan.fields.push(&bytes)?;
        }
        if scan.offset < e.length as u64 {
            return Ok(None);
        }
        if scan.hash.finish().to_string() != e.digest {
            return Err(error("display record digest mismatch"));
        }
        if e.length <= 256 * 1024 {
            return selected(&scan.small).map(Some);
        }
        let mut value = scan.fields.finish()?;
        if matches!(scan.target, DisplayRecord::Message(_)) {
            super::mark_old_hidden_text(&mut value, e);
        }
        Ok(Some(value))
    }
}

#[derive(Default)]
pub(super) struct Fields {
    display: bool,
    state: State,
    root: serde_json::Map<String, Value>,
    stage: serde_json::Map<String, Value>,
    stages: Vec<Value>,
    in_stage: bool,
    key: Vec<u8>,
    key_escape: bool,
    name: String,
    value: ValueScan,
    capture: bool,
    legacy: bool,
    retained: usize,
}
#[derive(Default, PartialEq)]
enum State {
    #[default]
    Start,
    Key,
    KeyString,
    Colon,
    Value,
    After,
    StageElement,
    StageAfter,
    Done,
}
#[derive(Default)]
struct ValueScan {
    depth: Vec<u8>,
    string: bool,
    escape: bool,
    unicode: u8,
    primitive: bool,
    bytes: Vec<u8>,
    started: bool,
    ignored_depth: u64,
    raw_images: bool,
}
impl ValueScan {
    // 0: continue, 1: complete and consumed, 2: complete before delimiter.
    fn byte(&mut self, byte: u8, capture: bool, left: usize) -> Result<u8> {
        if self.started && self.primitive && (byte.is_ascii_whitespace() || matches!(byte, b',' | b'}' | b']')) {
            return Ok(2);
        }
        if capture {
            if self.bytes.len() >= left {
                return Err(error("display selected fields exceed 1 MiB"));
            }
            self.bytes.push(byte);
        }
        if self.string {
            if self.unicode > 0 {
                if !byte.is_ascii_hexdigit() {
                    return Err(error("invalid display string escape"));
                }
                self.unicode -= 1;
            } else if self.escape {
                self.escape = false;
                if byte == b'u' {
                    self.unicode = 4;
                } else if !matches!(byte, b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') {
                    return Err(error("invalid display string escape"));
                }
            } else if byte == b'\\' {
                self.escape = true;
            } else if byte == b'"' {
                self.string = false;
                if self.depth.is_empty() && self.ignored_depth == 0 {
                    return Ok(1);
                }
            } else if byte < 32 {
                return Err(error("invalid display string"));
            }
        } else {
            match byte {
                b'"' => self.string = true,
                b'{' | b'[' => {
                    if !capture {
                        self.ignored_depth = self.ignored_depth.checked_add(1).ok_or_else(|| error("display nesting overflow"))?;
                    } else {
                        if self.depth.len() >= 128 && !self.raw_images {
                            return Err(error("display nesting exceeds bound"));
                        }
                        self.depth.push(byte);
                    }
                }
                b'}' | b']' => {
                    let expected = if byte == b'}' { b'{' } else { b'[' };
                    // A retained legacy setting can stop buffering halfway through
                    // its value. Close ignored descendants first, then the captured
                    // ancestors that were already open at the size boundary.
                    if self.ignored_depth > 0 {
                        self.ignored_depth -= 1;
                    } else if self.depth.pop() != Some(expected) {
                        return Err(error("invalid display nesting"));
                    }
                    if self.depth.is_empty() && self.ignored_depth == 0 {
                        return Ok(1);
                    }
                }
                _ if !self.started => {
                    if !matches!(byte, b'-' | b'0'..=b'9' | b't' | b'f' | b'n') {
                        return Err(error("invalid display value"));
                    }
                    self.primitive = true;
                }
                _ => {}
            }
        }
        self.started = true;
        Ok(0)
    }
}
impl Fields {
    fn save(&mut self) -> Result<()> {
        if !self.in_stage && self.name == "backend_terminal" {
            // Recovery needs presence, never the complete terminal payload.
            // Keep additive data streaming instead of giving it a display quota.
            self.root.insert(self.name.clone(), Value::Bool(true));
        }
        if self.capture {
            self.retained += self.value.bytes.len();
            let value = if self.value.raw_images {
                let mut shell = b"{\"images\":".to_vec();
                shell.extend_from_slice(&self.value.bytes);
                shell.push(b'}');
                migration::display_images(&shell)?.unwrap_or(Value::Null)
            } else {
                serde_json::from_slice(&self.value.bytes).map_err(error)?
            };
            if self.in_stage {
                self.stage.insert(self.name.clone(), value);
            } else {
                self.root.insert(self.name.clone(), value);
            }
        }
        self.value = ValueScan::default();
        self.state = State::After;
        Ok(())
    }
    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<()> {
        let mut offset = 0;
        while let Some(&b) = bytes.get(offset) {
            if self.state != State::KeyString && self.state != State::Value && b.is_ascii_whitespace() {
                offset += 1;
                continue;
            }
            let mut consumed = true;
            match self.state {
                State::Start => {
                    if b != b'{' {
                        return Err(error("display record must be an object"));
                    }
                    self.state = State::Key;
                }
                State::Key => {
                    if b == b'}' {
                        self.close_map()?;
                    } else if b == b'"' {
                        self.key.clear();
                        self.key.push(b);
                        self.key_escape = false;
                        self.state = State::KeyString;
                    } else {
                        return Err(error("invalid display record key"));
                    }
                }
                State::KeyString => {
                    if self.key.len() >= 256 {
                        return Err(error("display key exceeds bound"));
                    }
                    self.key.push(b);
                    if self.key_escape {
                        self.key_escape = false;
                    } else if b == b'\\' {
                        self.key_escape = true;
                    } else if b == b'"' {
                        self.name = serde_json::from_slice::<String>(&self.key).map_err(error)?;
                        if self.name.len() > 128 {
                            return Err(error("display key exceeds bound"));
                        }
                        self.state = State::Colon;
                    }
                }
                State::Colon => {
                    if b != b':' {
                        return Err(error("display field separator missing"));
                    }
                    self.state = State::Value;
                }
                State::Value => {
                    if !self.value.started && b.is_ascii_whitespace() {
                        offset += 1;
                        continue;
                    }
                    if !self.in_stage && self.name == "stages" && !self.value.started {
                        if b != b'[' {
                            return Err(error("display stages must be an array"));
                        }
                        self.state = State::StageElement;
                    } else {
                        if !self.value.started {
                            self.capture = if self.in_stage {
                                matches!(
                                    self.name.as_str(),
                                    "stage"
                                        | "plan_id"
                                        | "reason"
                                        | "execution_outcome"
                                        | "delivery_state"
                                        | "locally_published"
                                        | "preview_message_id"
                                        | "display_before"
                                        | "display_target"
                                        | "display_actual"
                                        | "before_state"
                                        | "proposed_state"
                                        | "applied_state"
                                )
                            } else {
                                (self.display
                                    && matches!(
                                        self.name.as_str(),
                                        "role" | "text" | "tool_calls" | "tool_call_id" | "complete_state" | "display_kind"
                                    ))
                                    || matches!(
                                        self.name.as_str(),
                                        "local_round_id"
                                            | "tool_call_id"
                                            | "tool_request_id"
                                            | "tool_name"
                                            | "stage"
                                            | "model_call_id"
                                            | "display_outcome"
                                            | "display_elapsed_us"
                                            | "display_write_state"
                                            | "display_delivery_state"
                                            | "display_plan_id"
                                            | "role"
                                            | "display_kind"
                                            | "complete_state"
                                            | "authored_text_bytes"
                                            | "local_message_id"
                                            | "photo_id"
                                            | "local_terminal"
                                            | "final_message_id"
                                            | "terminal_notice"
                                            | "phase"
                                            | "request_text"
                                            | "submission_seq"
                                            | "connection_generation"
                                            | "runtime_library_generation"
                                            | "backend_round_id"
                                            | "trace_id"
                                            | "before_preview_message_id"
                                            | "recent_complete_hash"
                                            | "recent_complete_result"
                                            | "revert_state"
                                            | "plan_id"
                                            | "round_id"
                                            | "reason"
                                            | "operations"
                                    )
                                    || (!self.display && self.name == "images")
                            };
                        }
                        self.value.raw_images = !self.in_stage && self.name == "images";
                        self.legacy = matches!(self.name.as_str(), "before_state" | "proposed_state" | "applied_state" | "recent_complete_result")
                            || self.value.raw_images;
                        // Current submissions are at most 4096 UTF-8 bytes.
                        // A larger legacy request stays in its original record
                        // and readable message; it cannot consume the card budget.
                        if !self.in_stage && self.name == "request_text" && self.value.bytes.len() >= 32 * 1024 {
                            self.capture = false;
                            self.value.bytes.clear();
                        }
                        // Compatibility settings can contain arbitrary old mask geometry.
                        // Retain a small complete setting only; otherwise keep scanning
                        // without a buffer and use the execution-time display fields.
                        if self.legacy && self.value.bytes.len() >= 64 * 1024 {
                            self.capture = false;
                            self.value.bytes.clear();
                            if self.in_stage {
                                self.stage.insert("parameter_details_unavailable".into(), Value::Bool(true));
                            }
                        }
                        // Diagnostics are optional and small. Continue validating the complete
                        // record/digest while ignoring an oversized diagnostic value.
                        if !self.in_stage
                            && matches!(self.name.as_str(), "trace_id" | "final_message_id" | "terminal_notice")
                            && self.value.bytes.len() >= 4096
                        {
                            self.capture = false;
                            self.value.bytes.clear();
                        }
                        match self.value.byte(b, self.capture, 1024 * 1024 - self.retained)? {
                            1 => self.save()?,
                            2 => {
                                self.save()?;
                                consumed = false;
                            }
                            _ => {}
                        }
                    }
                }
                State::After => {
                    if b == b',' {
                        self.state = State::Key;
                    } else if b == b'}' {
                        self.close_map()?;
                    } else {
                        return Err(error("invalid display field boundary"));
                    }
                }
                State::StageElement => {
                    if b == b']' {
                        self.root.insert("stages".into(), Value::Array(std::mem::take(&mut self.stages)));
                        self.state = State::After;
                    } else if b == b'{' {
                        if self.stages.len() >= 64 {
                            return Err(error("display stage count exceeds bound"));
                        }
                        self.in_stage = true;
                        self.state = State::Key;
                    } else {
                        return Err(error("invalid display stage"));
                    }
                }
                State::StageAfter => {
                    if b == b',' {
                        self.state = State::StageElement;
                    } else if b == b']' {
                        self.root.insert("stages".into(), Value::Array(std::mem::take(&mut self.stages)));
                        self.state = State::After;
                    } else {
                        return Err(error("invalid display stages boundary"));
                    }
                }
                State::Done => return Err(error("trailing display record bytes")),
            }
            if consumed {
                offset += 1;
            }
        }
        Ok(())
    }
    fn close_map(&mut self) -> Result<()> {
        if self.in_stage {
            self.stages.push(Value::Object(std::mem::take(&mut self.stage)));
            self.in_stage = false;
            self.state = State::StageAfter;
        } else {
            self.state = State::Done;
        }
        Ok(())
    }
    pub(super) fn finish(&mut self) -> Result<Value> {
        if self.state != State::Done {
            return Err(error("incomplete display record"));
        }
        Ok(Value::Object(std::mem::take(&mut self.root)))
    }
}

/// Known selected fields are bounded; additive values never become a Value.
pub(super) fn selected(bytes: &[u8]) -> Result<Value> {
    match serde_json::from_slice(bytes) {
        Ok(value) => Ok(value),
        Err(_) => {
            let mut fields = Fields { display: true, ..Default::default() };
            fields.push(bytes)?;
            let mut value = fields.finish()?;
            if let Some(images) = migration::display_images(bytes)? {
                value["images"] = images;
            }
            Ok(value)
        }
    }
}
