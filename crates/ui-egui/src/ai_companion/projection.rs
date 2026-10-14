//! Model input is a validated protocol projection, never the local display record.
use super::*;
const MAX_TEXT: usize = 256 * 1024;
const MAX_REQUEST: usize = 32 * 1024 * 1024;
const MAX_IMAGE: usize = 128 * 1024;
pub(super) fn invalid() -> String {
    tr("The AI history is incomplete or invalid. Local history is retained. Repair attachments or delete the conversation to retry.").into()
}
fn limit() -> String {
    tr("The complete AI history exceeds the service limit. Local history is retained.").into()
}
pub(super) struct HistoryProjection {
    photo: PhotoId,
    messages: Vec<Value>,
    pending: BTreeSet<String>,
    used: BTreeSet<String>,
    text: usize,
    images: usize,
    bytes: usize,
}
impl HistoryProjection {
    pub(super) fn new(photo: PhotoId) -> Self {
        Self { photo, messages: Vec::new(), pending: BTreeSet::new(), used: BTreeSet::new(), text: 0, images: 0, bytes: 0 }
    }
    pub(super) fn push(&mut self, source: &Value, attachment: impl FnMut(&Value) -> Result<Vec<u8>, String>) -> Result<(), String> {
        self.push_cancellable(source, attachment, || false)
    }
    pub(super) fn push_cancellable(
        &mut self,
        source: &Value,
        mut attachment: impl FnMut(&Value) -> Result<Vec<u8>, String>,
        cancelled: impl Fn() -> bool,
    ) -> Result<(), String> {
        if cancelled() {
            return Err("cancelled".into());
        }
        let object = source.as_object().ok_or_else(invalid)?;
        if let Some(complete) = object.get("complete_state") {
            match complete.as_str() {
                Some("complete") => {}
                Some("streaming" | "incomplete" | "pending") if source["role"] == "assistant" => return Ok(()),
                _ => return Err(invalid()),
            }
        }
        if matches!(source.get("display_kind").and_then(Value::as_str), Some("system_notice" | "task_status")) && object.get("role").is_none() {
            return Ok(());
        }
        let role = source.get("role").and_then(Value::as_str).ok_or_else(invalid)?;
        if !matches!(role, "user" | "assistant" | "tool") || (!self.pending.is_empty() && role != "tool") {
            return Err(invalid());
        }
        // The backend appends the actual current photo request. Reserve its slot/image now.
        if self.messages.len() >= 1023 {
            return Err(limit());
        }
        let text = match source.get("text") {
            None | Some(Value::Null) => "",
            Some(value) => value.as_str().ok_or_else(invalid)?,
        };
        self.text = self.text.checked_add(text.len()).ok_or_else(limit)?;
        let mut wire = json!({"role":role,"text":text});
        let result_id = match source.get("tool_call_id") {
            None | Some(Value::Null) => "",
            Some(value) => value.as_str().ok_or_else(invalid)?,
        };
        if role == "tool" {
            if !self.pending.remove(result_id) {
                return Err(invalid());
            }
            wire["tool_call_id"] = json!(result_id);
        } else if !result_id.is_empty() {
            return Err(invalid());
        }
        if let Some(calls) = source.get("tool_calls").filter(|v| !v.is_null()) {
            let calls = calls.as_array().ok_or_else(invalid)?;
            if calls.len() > 20 || (role != "assistant" && !calls.is_empty()) {
                return Err(invalid());
            }
            let mut projected = Vec::new();
            for call in calls {
                let id = call.get("id").and_then(Value::as_str).filter(|v| !v.is_empty() && v.len() <= 256).ok_or_else(invalid)?;
                let name = call.get("name").and_then(Value::as_str).filter(|v| !v.is_empty() && v.len() <= 128).ok_or_else(invalid)?;
                let args = call.get("arguments").ok_or_else(invalid)?;
                let arg_size = size(args)?;
                if arg_size > 64 * 1024 || !self.used.insert(id.to_owned()) {
                    return Err(invalid());
                }
                self.text = self.text.checked_add(arg_size).ok_or_else(limit)?;
                self.pending.insert(id.to_owned());
                projected.push(json!({"id":id,"name":name,"arguments":args}));
            }
            if !projected.is_empty() {
                wire["tool_calls"] = json!(projected);
            }
        }
        if self.text > MAX_TEXT {
            return Err(limit());
        }
        if let Some(images) = source.get("images").filter(|v| !v.is_null()) {
            let images = images.as_array().ok_or_else(invalid)?;
            if !images.is_empty() && role == "assistant" {
                return Err(invalid());
            }
            self.images = self.images.checked_add(images.len()).ok_or_else(limit)?;
            if self.images >= 256 {
                return Err(limit());
            }
            let mut pictures = Vec::new();
            let mut picture_bytes = 0usize;
            for image in images {
                if cancelled() {
                    return Err("cancelled".into());
                }
                let mime = image.get("mime_type").and_then(Value::as_str).ok_or_else(invalid)?;
                if !matches!(mime, "image/jpeg" | "image/png") {
                    return Err(invalid());
                }
                if image.get("attachment").is_some() && image.get("data").is_some() {
                    return Err(invalid());
                }
                if let Some(name) = image.get("attachment").and_then(Value::as_str)
                    && (image.get("data").is_some()
                        || !(name.starts_with(&format!("companion/v2/photos/{}/previews/", self.photo.0))
                            || name.starts_with(&format!("companion/preview-{}-", self.photo.0))))
                {
                    return Err(invalid());
                }
                let encoded_bound = if let Some(encoded) = image.get("data").and_then(Value::as_str) {
                    encoded.len()
                } else {
                    let length = image.get("length").and_then(Value::as_u64).unwrap_or(MAX_IMAGE as u64);
                    if length > MAX_IMAGE as u64 {
                        return Err(limit());
                    }
                    usize::try_from(length).map_err(|_| limit())?.div_ceil(3).saturating_mul(4)
                };
                let image_size = encoded_bound.saturating_add(size(&json!({"mime_type":mime,"data":""}))?).saturating_add(1);
                if self.bytes.saturating_add(size(&wire)?).saturating_add(picture_bytes).saturating_add(image_size).saturating_add(16) > MAX_REQUEST {
                    return Err(limit());
                }
                let data = if let Some(encoded) = image.get("data") {
                    let encoded = encoded.as_str().ok_or_else(invalid)?;
                    if encoded.len() > MAX_IMAGE.div_ceil(3) * 4 {
                        return Err(limit());
                    }
                    Base64::decode_vec(encoded).map_err(|_| invalid())?
                } else if image.get("attachment").is_some() {
                    if image.get("length").and_then(Value::as_u64).is_some_and(|length| length > MAX_IMAGE as u64) {
                        return Err(limit());
                    }
                    attachment(image)?
                } else {
                    return Err(invalid());
                };
                if cancelled() {
                    return Err("cancelled".into());
                }
                if data.is_empty() || data.len() > MAX_IMAGE {
                    return Err(limit());
                }
                if let Some(name) = image.get("attachment").and_then(Value::as_str).filter(|name| name.starts_with("companion/preview-")) {
                    let prefix = format!("companion/preview-{}-", self.photo.0);
                    let recorded = name
                        .strip_prefix(&prefix)
                        .and_then(|tail| tail.strip_suffix(".bin"))
                        .filter(|hash| hash.len() == 32)
                        .and_then(|hash| u128::from_str_radix(hash, 16).ok())
                        .ok_or_else(invalid)?;
                    if recorded != lightcraft_preview::hash_bytes(&data).0 {
                        return Err(invalid());
                    }
                }
                if image.get("length").and_then(Value::as_u64).is_some_and(|length| length != data.len() as u64) {
                    return Err(invalid());
                }
                lightcraft_engine::companion::validate_history_image(mime, &data).map_err(|_| invalid())?;
                picture_bytes = picture_bytes
                    .saturating_add(data.len().div_ceil(3).saturating_mul(4))
                    .saturating_add(size(&json!({"mime_type":mime,"data":""}))?)
                    .saturating_add(1);
                pictures.push(json!({"mime_type":mime,"data":Base64::encode_string(&data)}));
            }
            if !pictures.is_empty() {
                wire["images"] = json!(pictures);
            }
        }
        self.bytes = self.bytes.checked_add(size(&wire)?.saturating_add(1)).ok_or_else(limit)?;
        if self.bytes > MAX_REQUEST {
            return Err(limit());
        }
        self.messages.push(wire);
        Ok(())
    }
    pub(super) fn finish(self) -> Result<Vec<Value>, String> {
        if !self.pending.is_empty() {
            return Err(invalid());
        }
        Ok(self.messages)
    }
}
struct Counter(usize);
impl std::io::Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.checked_add(bytes.len()).ok_or_else(|| std::io::Error::other("request size overflow"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn size(value: &Value) -> Result<usize, String> {
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).map_err(|_| invalid())?;
    Ok(counter.0)
}
/// Include the exact non-image input envelope and reserve the largest valid current
/// JPEG before creating a backend round. No preview is uploaded during this check.
pub(super) fn reserve_input(history: &[Value], prompt: &str, settings: &DevelopSettings) -> Result<(), String> {
    let current = json!(settings);
    if size(&current)? > MAX_TEXT {
        return Err(limit());
    }
    let mut text = prompt.len().saturating_add("\nActual photo state: ".len()).saturating_add(size(&current)?);
    for message in history {
        text = text.saturating_add(message["text"].as_str().map(str::len).unwrap_or(0));
        for call in message["tool_calls"].as_array().into_iter().flatten() {
            text = text.saturating_add(size(&call["arguments"])?);
        }
    }
    if text > MAX_TEXT || history.len() >= 1024 {
        return Err(limit());
    }
    let envelope = json!({"input_id":"input-18446744073709551615","photo_revision":u64::MAX,"prompt":prompt,"current_state":settings,"preview":{"mime_type":"image/jpeg","width":640,"height":640,"frame_id":"x".repeat(128),"data_base64":""},"history":[],"capabilities":capabilities(settings)});
    let mut history_bytes = history.len().saturating_sub(1);
    for message in history {
        history_bytes = history_bytes.checked_add(size(message)?).ok_or_else(limit)?;
    }
    if size(&envelope)?.saturating_add(history_bytes).saturating_add(MAX_IMAGE.div_ceil(3) * 4) > MAX_REQUEST {
        return Err(limit());
    }
    Ok(())
}
