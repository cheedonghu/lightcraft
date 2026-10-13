//! Bounded native history I/O runs outside the frame and owns a captured graph.
use super::*;
use lightcraft_engine::companion::storage::{HistoryCursor, HistoryReader, HistorySlice};
struct ReadMessage {
    scope: PhotoScope,
    transaction: String,
    cursor: HistoryCursor,
    result: Result<HistorySlice, String>,
}
pub(super) struct Paging {
    tx: mpsc::SyncSender<ReadMessage>,
    rx: mpsc::Receiver<ReadMessage>,
    pub(super) alive: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}
impl Default for Paging {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(2);
        Self { tx, rx, alive: Arc::new(AtomicBool::new(false)), cancel: Arc::new(AtomicBool::new(false)) }
    }
}
impl Drop for Paging {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Paging {
    pub(super) fn busy(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
    pub(super) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl State {
    pub(super) fn poll_pages(&mut self, app: &mut LightcraftApp) {
        while let Ok(message) = self.paging.rx.try_recv() {
            if message.scope.library_generation != app.session.library_generation() {
                continue;
            }
            let Some(conversation) = self.conversations.get_mut(&message.scope.photo) else { continue };
            if conversation.scope != Some(message.scope) || conversation.prefix.as_ref().is_none_or(|h| h.transaction_id != message.transaction) {
                continue;
            }
            match message.result {
                Ok(page) => {
                    conversation.view = Some(HistoryView {
                        cursor: message.cursor,
                        next: page.next,
                        messages: page.records,
                        fragments: page.fragments,
                        bytes: page.bytes,
                    })
                }
                Err(error) => {
                    conversation.notice = error;
                    conversation.view =
                        Some(HistoryView { cursor: message.cursor, next: None, messages: Vec::new(), fragments: Vec::new(), bytes: 0 });
                }
            }
        }
    }
    pub(super) fn request_page(&mut self, app: &mut LightcraftApp, photo: PhotoId, cursor: HistoryCursor) -> Result<(), String> {
        let Some(header) = self.conversations.get(&photo).and_then(|c| c.prefix.clone()) else { return Ok(()) };
        if let Some(mut reader) = app.session.companion_reader(&header).map_err(|e| e.to_string())? {
            if self.paging.alive.load(Ordering::Relaxed) {
                return Ok(());
            }
            let scope = PhotoScope { library_generation: app.session.library_generation(), photo };
            let transaction = header.transaction_id;
            let tx = self.paging.tx.clone();
            self.paging.cancel = Arc::new(AtomicBool::new(false));
            let cancel = self.paging.cancel.clone();
            self.paging.alive.store(true, Ordering::Relaxed);
            let guard = WorkerAlive(self.paging.alive.clone());
            std::thread::Builder::new()
                .name("companion-history-page".into())
                .spawn(move || {
                    let _guard = guard;
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let result = reader.slice(cursor, 256 * 1024).map_err(|e| e.to_string());
                    if !cancel.load(Ordering::Relaxed) {
                        let _ = tx.try_send(ReadMessage { scope, transaction, cursor, result });
                    }
                })
                .map_err(|e| e.to_string())?;
        } else {
            self.read_page(app, photo, cursor)?;
        }
        Ok(())
    }
    pub(super) fn begin_history(
        &mut self,
        mut reader: HistoryReader,
        identity: RoundIdentity,
        tail: Vec<Value>,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let tx = self.tx.clone();
        let photo = identity.scope.photo;
        let alive = Arc::new(AtomicBool::new(true));
        let guard = WorkerAlive(alive.clone());
        std::thread::Builder::new()
            .name("companion-model-history".into())
            .spawn(move || {
                let _guard = guard;
                let result = assemble(&mut reader, tail, &cancel);
                if !cancel.load(Ordering::Relaxed) && tx.try_send(WorkerMessage::HistoryPrepared { identity, result }).is_err() {
                    cancel.store(true, Ordering::Relaxed);
                }
            })
            .map_err(|e| e.to_string())?;
        self.workers.insert(photo, alive);
        Ok(())
    }
}
fn assemble(reader: &mut HistoryReader, tail: Vec<Value>, cancel: &AtomicBool) -> Result<Vec<Value>, String> {
    if reader.header().message_count.saturating_add(tail.len() as u64) > 1024 {
        return Err(tr("The complete AI history exceeds the service limit. Local history is retained.").into());
    }
    let mut messages = Vec::new();
    let mut text = 0usize;
    let mut bytes = 0usize;
    let mut images = 0usize;
    let mut cursor = 0;
    while cursor < reader.header().message_count {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let page = reader.messages(cursor, 1, (32 * 1024 * 1024usize).saturating_sub(bytes)).map_err(|e| e.to_string())?;
        for message in page.records {
            append(reader, message, &mut messages, &mut text, &mut bytes, &mut images, cancel)?;
        }
        cursor = page.next_cursor.unwrap_or(reader.header().message_count);
    }
    for message in tail {
        append(reader, message, &mut messages, &mut text, &mut bytes, &mut images, cancel)?;
    }
    Ok(messages)
}
fn append(
    reader: &mut HistoryReader,
    mut message: Value,
    messages: &mut Vec<Value>,
    text: &mut usize,
    bytes: &mut usize,
    images: &mut usize,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    if let Some(object) = message.as_object_mut() {
        for key in ["local_round_id", "local_message_id", "stage", "model_call_id", "tool_request_id", "complete_state", "display_kind"] {
            object.remove(key);
        }
    }
    *text = text.saturating_add(message.get("text").and_then(Value::as_str).map(str::len).unwrap_or(0));
    if let Some(calls) = message.get("tool_calls") {
        *text = text.saturating_add(lightcraft_engine::companion::storage::record_size(calls).map_err(|e| e.to_string())?);
    }
    if *text > 256 * 1024 {
        return Err(tr("The complete AI history exceeds the service limit. Local history is retained.").into());
    }
    let mut projected_size = lightcraft_engine::companion::storage::record_size(&message).map_err(|e| e.to_string())?;
    if let Some(pictures) = message.get_mut("images").and_then(Value::as_array_mut) {
        *images = images.saturating_add(pictures.len());
        if *images > 256 {
            return Err(tr("The complete AI history exceeds the service limit. Local history is retained.").into());
        }
        for picture in pictures {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            if picture.get("attachment").is_some() {
                let stored_size = lightcraft_engine::companion::storage::record_size(picture).map_err(|e| e.to_string())?;
                let length = picture.get("length").and_then(Value::as_u64).ok_or("missing preview length")?;
                if length > 128 * 1024 {
                    return Err("invalid preview length".into());
                }
                let encoded = usize::try_from(length.div_ceil(3).saturating_mul(4)).map_err(|e| e.to_string())?;
                projected_size = projected_size.saturating_sub(stored_size).saturating_add(encoded).saturating_add(128);
                if bytes.saturating_add(projected_size) > 32 * 1024 * 1024 {
                    return Err(tr("The complete AI history exceeds the service limit. Local history is retained.").into());
                }
                let data = reader.preview(picture).map_err(|e| e.to_string())?;
                let object = picture.as_object_mut().ok_or("invalid image")?;
                for key in ["attachment", "digest", "length", "width", "height"] {
                    object.remove(key);
                }
                picture["data"] = json!(Base64::encode_string(&data));
            }
        }
    }
    *bytes = bytes.saturating_add(lightcraft_engine::companion::storage::record_size(&message).map_err(|e| e.to_string())?);
    if *bytes > 32 * 1024 * 1024 {
        return Err(tr("The complete AI history exceeds the service limit. Local history is retained.").into());
    }
    messages.push(message);
    Ok(())
}
