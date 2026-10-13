//! Bounded native history I/O runs outside the frame and owns a captured graph.
use super::*;
use lightcraft_engine::companion::storage::{HistoryCursor, HistoryReader, HistorySlice};
#[derive(Clone)]
struct PageRequest {
    id: u64,
    scope: PhotoScope,
    header: lightcraft_engine::companion::storage::PhotoHeader,
    cursor: HistoryCursor,
}
struct ReadMessage {
    request: u64,
    scope: PhotoScope,
    transaction: String,
    cursor: HistoryCursor,
    result: Result<(HistorySlice, Vec<card::Card>), String>,
}
pub(super) struct Paging {
    tx: mpsc::SyncSender<ReadMessage>,
    rx: mpsc::Receiver<ReadMessage>,
    pub(super) alive: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    desired: Option<PageRequest>,
    pending: Option<PageRequest>,
    sequence: u64,
}
impl Default for Paging {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(2);
        Self { tx, rx, alive: Arc::new(AtomicBool::new(false)), cancel: Arc::new(AtomicBool::new(false)), desired: None, pending: None, sequence: 0 }
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
    pub(super) fn has_request(&self, photo: PhotoId, transaction: &str) -> bool {
        self.desired.as_ref().is_some_and(|r| r.scope.photo == photo && r.header.transaction_id == transaction)
    }
    pub(super) fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.desired = None;
        self.pending = None;
    }
}
impl State {
    pub(super) fn poll_pages(&mut self, app: &mut LightcraftApp) {
        while let Ok(message) = self.paging.rx.try_recv() {
            if message.scope.library_generation != app.session.library_generation()
                || self.paging.desired.as_ref().is_none_or(|r| r.id != message.request)
            {
                continue;
            }
            let Some(conversation) = self.conversations.get_mut(&message.scope.photo) else { continue };
            if conversation.scope != Some(message.scope) || conversation.prefix.as_ref().is_none_or(|h| h.transaction_id != message.transaction) {
                continue;
            }
            match message.result {
                Ok((page, cards)) => {
                    conversation.view = Some(HistoryView {
                        items: page.items,
                        cursor: message.cursor,
                        next: page.next,
                        messages: page.records,
                        fragments: page.fragments,
                        bytes: page.bytes,
                        cards,
                        card_cursor: None,
                        card_pending: BTreeSet::new(),
                    });
                }
                Err(error) => {
                    conversation.notice = error;
                    conversation.view = Some(HistoryView {
                        items: Vec::new(),
                        cursor: message.cursor,
                        next: None,
                        messages: Vec::new(),
                        fragments: Vec::new(),
                        bytes: 0,
                        cards: Vec::new(),
                        card_cursor: None,
                        card_pending: BTreeSet::new(),
                    });
                }
            }
        }
        // Adapters without a background handle advance one bounded scan page
        // per frame; they never synchronously hydrate a whole conversation.
        let step = app.session.active().and_then(|photo| {
            self.conversations
                .get(&photo)
                .and_then(|c| c.view.as_ref().and_then(|v| v.card_cursor.map(|cursor| (photo, c.prefix.clone(), cursor, v.card_pending.clone()))))
        });
        if let Some((photo, Some(header), cursor, _wanted)) = step {
            let result = app.session.companion_rounds_display_at(&header, cursor, 32);
            if let Some(conversation) = self.conversations.get_mut(&photo)
                && let Some(view) = conversation.view.as_mut()
            {
                match result {
                    Ok(page) => {
                        view.card_cursor = page.next_cursor;
                        for record in page.records {
                            if record["local_round_id"].as_str().is_some_and(|id| view.card_pending.remove(id))
                                && let Some(card) = card::from_record(&record, None)
                                && !view.cards.iter().any(|c| c.local_round_id == card.local_round_id)
                            {
                                view.cards.push(card);
                            }
                        }
                        if view.card_pending.is_empty() {
                            view.card_cursor = None;
                        }
                    }
                    Err(error) => {
                        conversation.notice = error.to_string();
                        view.card_cursor = None;
                    }
                }
            }
        }
        if !self.paging.busy()
            && let Some(request) = self.paging.pending.take()
            && request.scope.library_generation == app.session.library_generation()
            && self
                .conversations
                .get(&request.scope.photo)
                .is_some_and(|c| c.scope == Some(request.scope) && c.prefix.as_ref() == Some(&request.header))
            && let Err(error) = self.launch_page(app, request.clone())
            && let Some(c) = self.conversations.get_mut(&request.scope.photo)
        {
            c.notice = error;
        }
    }
    pub(super) fn request_page(&mut self, app: &mut LightcraftApp, photo: PhotoId, cursor: HistoryCursor) -> Result<(), String> {
        let Some(header) = self.conversations.get(&photo).and_then(|c| c.prefix.clone()) else { return Ok(()) };
        if self.paging.busy() && self.paging.desired.as_ref().is_some_and(|r| r.scope.photo == photo && r.header == header && r.cursor == cursor) {
            return Ok(());
        }
        self.paging.sequence = self.paging.sequence.checked_add(1).ok_or_else(|| tr("AI history preparation failed. Try again.").to_owned())?;
        let request = PageRequest {
            id: self.paging.sequence,
            scope: PhotoScope { library_generation: app.session.library_generation(), photo },
            header,
            cursor,
        };
        self.paging.desired = Some(request.clone());
        if self.paging.busy() {
            // Keep one latest desired cursor. Retire the existing reader before
            // opening another handle; its already-queued result has an obsolete id.
            self.paging.cancel.store(true, Ordering::Relaxed);
            self.paging.pending = Some(request);
            return Ok(());
        }
        self.paging.pending = None;
        self.launch_page(app, request)
    }
    fn launch_page(&mut self, app: &mut LightcraftApp, request: PageRequest) -> Result<(), String> {
        // The preceding reader has retired. Drain its obsolete completions before
        // launching the sole next reader so stale pages cannot fill the channel.
        while self.paging.rx.try_recv().is_ok() {}
        if let Some(mut reader) = app.session.companion_reader(&request.header).map_err(|e| e.to_string())? {
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
                    let result = reader.slice(request.cursor, 256 * 1024).map_err(|e| e.to_string()).and_then(|page| {
                        let mut cursor = 0;
                        let mut cards = Vec::new();
                        // Exact recorded ids, never inferred from neighbouring rounds.
                        let mut wanted: BTreeSet<_> = page.records.iter().filter_map(|m| m["local_round_id"].as_str()).map(str::to_owned).collect();
                        for item in &page.items {
                            if cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            if let lightcraft_engine::companion::storage::HistoryItem::Fragment { record, .. } = item
                                && let Some(id) = reader.message_round_id(*record).map_err(|e| e.to_string())?
                            {
                                wanted.insert(id);
                            }
                        }
                        while !wanted.is_empty() && !cancel.load(Ordering::Relaxed) {
                            let rounds = reader.rounds_display(cursor, 32).map_err(|e| e.to_string())?;
                            for record in rounds.records {
                                if record["local_round_id"].as_str().is_some_and(|id| wanted.remove(id))
                                    && let Some(card) = card::from_record(&record, None)
                                {
                                    cards.push(card);
                                }
                            }
                            if wanted.is_empty() {
                                break;
                            }
                            let Some(next) = rounds.next_cursor else { break };
                            cursor = next;
                        }
                        Ok((page, cards))
                    });
                    if !cancel.load(Ordering::Relaxed) {
                        let _ = tx.try_send(ReadMessage {
                            request: request.id,
                            scope: request.scope,
                            transaction: request.header.transaction_id,
                            cursor: request.cursor,
                            result,
                        });
                    }
                })
                .map_err(|e| e.to_string())?;
        } else {
            self.read_page(app, request.scope.photo, request.cursor)?;
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
