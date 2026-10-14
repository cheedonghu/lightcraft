//! Bounded native history I/O runs outside the frame and owns a captured graph.
use super::*;
use lightcraft_engine::companion::storage::{DisplayRecord, DisplayScan, HistoryCursor, HistoryReader, HistorySlice};
#[derive(Clone)]
struct PageRequest {
    id: u64,
    scope: PhotoScope,
    header: lightcraft_engine::companion::storage::PhotoHeader,
    cursor: HistoryCursor,
    legacy_cursor: u64,
    preview_cursor: u64,
    presentation: BTreeMap<u64, Value>,
}
struct ReadMessage {
    retention: lightcraft_engine::companion::storage::HistoryRetention,
    request: u64,
    scope: PhotoScope,
    header: lightcraft_engine::companion::storage::PhotoHeader,
    transaction: String,
    cursor: HistoryCursor,
    result: ReadContent,
}
enum ReadContent {
    Page(
        Result<
            (Box<HistorySlice>, Vec<card::Card>, Option<String>, Option<String>, Option<Value>, Option<historical::Preview>, BTreeMap<u64, Value>),
            String,
        >,
    ),
    Preview(Option<historical::Preview>),
}
pub(super) struct Paging {
    tx: mpsc::SyncSender<ReadMessage>,
    rx: mpsc::Receiver<ReadMessage>,
    pub(super) alive: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    desired: Option<PageRequest>,
    pending: Option<PageRequest>,
    sequence: u64,
    completed: u64,
}
impl Default for Paging {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(2);
        Self {
            tx,
            rx,
            alive: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            desired: None,
            pending: None,
            sequence: 0,
            completed: 0,
        }
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
    pub(super) fn awaiting_page(&self, photo: PhotoId) -> bool {
        self.desired.as_ref().is_some_and(|request| request.scope.photo == photo && self.completed != request.id)
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
            self.paging.completed = message.request;
            match message.result {
                ReadContent::Page(Ok((page, cards, notice, diagnostic, legacy_plan, preview, fragment_messages))) => {
                    if let Some(notice) = notice {
                        conversation.notice = notice;
                    }
                    conversation.view = Some(HistoryView {
                        captured_header: message.header,
                        _retention: message.retention,
                        items: page.items,
                        cursor: message.cursor,
                        next: page.next,
                        messages: page.records,
                        fragments: page.fragments,
                        bytes: page.bytes,
                        cards_bytes: fragment_messages
                            .values()
                            .map(|v| lightcraft_engine::companion::storage::record_size(v).unwrap_or(usize::MAX))
                            .fold(0usize, usize::saturating_add)
                            .saturating_add(
                                cards.iter().map(|c| serde_json::to_vec(c).map_or(usize::MAX, |bytes| bytes.len())).fold(0, usize::saturating_add),
                            )
                            .saturating_add(legacy_plan.as_ref().and_then(|p| serde_json::to_vec(p).ok()).map_or(0, |p| p.len()))
                            .saturating_add(preview.as_ref().map_or(0, historical::Preview::bytes)),
                        cards,
                        fragment_messages,
                        diagnostic,
                        legacy_plan,
                        legacy_scan: None,
                        preview,
                        card_cursor: None,
                        card_pending: BTreeSet::new(),
                        card_owners: VecDeque::new(),
                        card_scan: None,
                    });
                    self.accept_page(app, message.scope.photo, message.cursor);
                }
                ReadContent::Preview(preview) => {
                    if let Some(view) = conversation.view.as_mut()
                        && view.captured_header == message.header
                        && view.cursor == message.cursor
                    {
                        view.cards_bytes = view.cards_bytes.saturating_sub(view.preview.as_ref().map_or(0, historical::Preview::bytes));
                        view.cards_bytes = view.cards_bytes.saturating_add(preview.as_ref().map_or(0, historical::Preview::bytes));
                        view.preview = preview;
                    }
                }
                ReadContent::Page(Err(error)) => {
                    conversation.notice = error;
                    conversation.view = Some(HistoryView {
                        captured_header: message.header,
                        _retention: message.retention,
                        items: Vec::new(),
                        cursor: message.cursor,
                        next: None,
                        messages: Vec::new(),
                        fragments: Vec::new(),
                        fragment_messages: BTreeMap::new(),
                        bytes: 0,
                        cards: Vec::new(),
                        diagnostic: None,
                        legacy_plan: None,
                        legacy_scan: None,
                        preview: None,
                        cards_bytes: 0,
                        card_cursor: None,
                        card_pending: BTreeSet::new(),
                        card_owners: VecDeque::new(),
                        card_scan: None,
                    });
                }
            }
        }
        if let Some(photo) = self.displayed_photo(app)
            && let Some(conversation) = self.conversations.get_mut(&photo)
            && let Some(view) = conversation.view.as_mut()
            && let Some(scan) = view.legacy_scan.as_mut()
        {
            match app.session.companion_display_step(scan) {
                Ok(Some(plan)) => {
                    view.cards_bytes = view.cards_bytes.saturating_add(serde_json::to_vec(&plan).map_or(usize::MAX, |b| b.len()));
                    view.legacy_plan = Some(plan);
                    view.legacy_scan = None;
                }
                Ok(None) => return,
                Err(e) => {
                    conversation.notice = e.to_string();
                    view.legacy_scan = None;
                }
            }
        }
        // One resumable record step per frame, retaining verified graph identity.
        if let Some(photo) = self.displayed_photo(app)
            && let Some(conversation) = self.conversations.get_mut(&photo)
            && let Some(header) = conversation.prefix.as_ref()
            && let Some(view) = conversation.view.as_mut()
            && let Some(cursor) = view.card_cursor
        {
            if view.card_scan.is_none() {
                let target = view.card_owners.front().copied().map(DisplayRecord::Message).unwrap_or(DisplayRecord::Round(cursor));
                view.card_scan = Some(DisplayScan::new(header.clone(), target));
            }
            if let Some(scan) = view.card_scan.as_mut() {
                match app.session.companion_display_step(scan) {
                    Ok(Some(record)) => {
                        match scan.target() {
                            DisplayRecord::Message(_) => {
                                if let Some(owner) = view.card_owners.pop_front() {
                                    let mut presentation = record.clone();
                                    if let Some(object) = presentation.as_object_mut() {
                                        object.retain(|key, _| {
                                            matches!(
                                                key.as_str(),
                                                "role"
                                                    | "display_kind"
                                                    | "authored_text_bytes"
                                                    | "legacy_hidden_text"
                                                    | "local_message_id"
                                                    | "local_round_id"
                                                    | "images"
                                            )
                                        });
                                    }
                                    view.cards_bytes = view
                                        .cards_bytes
                                        .saturating_add(lightcraft_engine::companion::storage::record_size(&presentation).unwrap_or(usize::MAX));
                                    view.fragment_messages.insert(owner, presentation);
                                }
                                if let Some(id) = record["local_round_id"].as_str().filter(|id| !id.is_empty() && id.len() <= 128) {
                                    view.card_pending.insert(id.to_owned());
                                }
                            }
                            DisplayRecord::Round(index) => {
                                let total = if header.record_count > 0 { header.record_count } else { header.round_count };
                                view.card_cursor = (index + 1 < total).then_some(index + 1);
                                if record["local_round_id"].as_str().is_some_and(|id| view.card_pending.remove(id)) {
                                    if let Some(trace) = errors::trace(&record) {
                                        view.diagnostic = Some(trace.to_owned());
                                    }
                                    if let Some(card) = card::from_record(&record, None)
                                        && !view.cards.iter().any(|c| c.local_round_id == card.local_round_id)
                                    {
                                        view.cards_bytes =
                                            view.cards_bytes.saturating_add(serde_json::to_vec(&card).map_or(usize::MAX, |bytes| bytes.len()));
                                        view.cards.push(card);
                                    }
                                }
                            }
                            DisplayRecord::LegacyPlan(_) => {
                                view.legacy_plan = Some(record);
                            }
                        }
                        view.card_scan = None;
                        if view.card_owners.is_empty() && view.card_pending.is_empty() {
                            view.card_cursor = None;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        conversation.notice = error.to_string();
                        view.card_cursor = None;
                        view.card_scan = None;
                    }
                }
            }
        }
        if let Some(photo) = self.displayed_photo(app)
            && self.bottom_reading.contains(&photo)
            && !self.paging.busy()
            && !self.paging.awaiting_page(photo)
            && let Some(view) = self.conversations.get(&photo).and_then(|c| c.view.as_ref())
            && view.card_cursor.is_none()
        {
            if let Some(next) = view.next {
                if let Err(error) = self.request_page(app, photo, next) {
                    self.text = error;
                    self.bottom_reading.remove(&photo);
                }
            } else {
                self.bottom_reading.remove(&photo);
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
            self.paging.completed = request.id;
            c.notice = error;
        }
    }
    pub(super) fn request_page(&mut self, app: &mut LightcraftApp, photo: PhotoId, cursor: HistoryCursor) -> Result<(), String> {
        let Some(header) = self.conversations.get(&photo).and_then(|c| c.prefix.clone()) else { return Ok(()) };
        if self.paging.busy()
            && self.paging.desired.as_ref().is_some_and(|r| {
                r.scope.photo == photo
                    && r.header == header
                    && r.cursor == cursor
                    && r.legacy_cursor == self.conversations.get(&photo).map(|c| c.legacy_cursor).unwrap_or(0)
                    && r.preview_cursor == self.conversations.get(&photo).map(|c| c.preview_cursor).unwrap_or(0)
            })
        {
            return Ok(());
        }
        self.paging.sequence = self.paging.sequence.checked_add(1).ok_or_else(|| tr("AI history preparation failed. Try again.").to_owned())?;
        let request = PageRequest {
            id: self.paging.sequence,
            scope: PhotoScope { library_generation: app.session.library_generation(), photo },
            header: header.clone(),
            cursor,
            legacy_cursor: self.conversations.get(&photo).map(|c| c.legacy_cursor).unwrap_or(0),
            preview_cursor: self.conversations.get(&photo).map(|c| c.preview_cursor).unwrap_or(0),
            presentation: self
                .conversations
                .get(&photo)
                .and_then(|c| c.view.as_ref())
                .filter(|v| v.captured_header == header)
                .map(|v| v.fragment_messages.clone())
                .unwrap_or_default(),
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
        let id = request.id;
        let result = self.launch_page(app, request);
        if result.is_err() {
            self.paging.completed = id;
        }
        result
    }
    fn launch_page(&mut self, app: &mut LightcraftApp, request: PageRequest) -> Result<(), String> {
        // The preceding reader has retired. Drain its obsolete completions before
        // launching the sole next reader so stale pages cannot fill the channel.
        while self.paging.rx.try_recv().is_ok() {}
        if let Some(reader) = app.session.companion_reader(&request.header).map_err(|e| e.to_string())? {
            let tx = self.paging.tx.clone();
            self.paging.cancel = Arc::new(AtomicBool::new(false));
            let cancel = self.paging.cancel.clone();
            self.paging.alive.store(true, Ordering::Relaxed);
            let guard = WorkerAlive(self.paging.alive.clone());
            std::thread::Builder::new()
                .name("companion-history-page".into())
                .spawn(move || {
                    let _guard = guard;
                    let mut reader = reader; // actual reader drops before alive on cancellation
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let result = reader.slice(request.cursor, 256 * 1024).map_err(|e| e.to_string()).map(|page| {
                        let mut cursor = 0;
                        let mut cards = Vec::new();
                        let mut notice = None;
                        let mut diagnostic = None;
                        let mut fragment_messages = BTreeMap::new();
                        // Exact recorded ids, never inferred from neighbouring rounds.
                        let mut wanted: BTreeSet<_> = page.records.iter().filter_map(|m| m["local_round_id"].as_str()).map(str::to_owned).collect();
                        for item in &page.items {
                            if cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            if let lightcraft_engine::companion::storage::HistoryItem::Fragment { record, .. } = item {
                                let selected = if let Some(message) = request.presentation.get(record) {
                                    Ok(message.clone())
                                } else {
                                    reader.message_display(*record)
                                };
                                match selected {
                                    Ok(message) => {
                                        if let Some(id) = message["local_round_id"].as_str() {
                                            wanted.insert(id.to_owned());
                                        }
                                        fragment_messages.insert(*record, message);
                                    }
                                    Err(error) => notice = Some(error.to_string()),
                                }
                            }
                        }
                        while !request.header.legacy_migrated && !wanted.is_empty() && !cancel.load(Ordering::Relaxed) {
                            let rounds = match reader.rounds_display(cursor, 32) {
                                Ok(rounds) => rounds,
                                Err(error) => {
                                    notice = Some(error.to_string());
                                    break;
                                }
                            };
                            for record in rounds.records {
                                if record["local_round_id"].as_str().is_some_and(|id| wanted.remove(id)) {
                                    if let Some(trace) = errors::trace(&record) {
                                        diagnostic = Some(trace.to_owned());
                                    }
                                    if let Some(card) = card::from_record(&record, None) {
                                        cards.push(card);
                                    }
                                }
                            }
                            if wanted.is_empty() {
                                break;
                            }
                            let Some(next) = rounds.next_cursor else { break };
                            cursor = next;
                        }
                        let legacy_plan = if request.header.legacy_plan_count() > 0 {
                            match reader.legacy_plan(request.legacy_cursor) {
                                Ok(plan) => Some(plan),
                                Err(e) => {
                                    notice = Some(e.to_string());
                                    None
                                }
                            }
                        } else {
                            None
                        };
                        let preview = if request.header.original_message_count() > 0 {
                            historical::read_native(&mut reader, &page, request.preview_cursor)
                        } else {
                            None
                        };
                        (page, cards, notice, diagnostic, legacy_plan, preview, fragment_messages)
                    });
                    if !cancel.load(Ordering::Relaxed) {
                        let _ = tx.try_send(ReadMessage {
                            retention: reader.into_retention(),
                            request: request.id,
                            scope: request.scope,
                            header: request.header.clone(),
                            transaction: request.header.transaction_id,
                            cursor: request.cursor,
                            result: ReadContent::Page(result.map(|(page, cards, notice, diagnostic, legacy_plan, preview, presentation)| {
                                (Box::new(page), cards, notice, diagnostic, legacy_plan, preview, presentation)
                            })),
                        });
                    }
                })
                .map_err(|e| e.to_string())?;
        } else {
            self.read_page(app, request.scope.photo, request.cursor)?;
            let page = self.conversations.get(&request.scope.photo).and_then(|c| c.view.as_ref()).map(|view| HistorySlice {
                items: view.items.clone(),
                records: view.messages.clone(),
                fragments: view.fragments.clone(),
                next: view.next,
                bytes: view.bytes,
            });
            if let Some(page) =
                page.filter(|page| request.header.original_message_count() > 0 && (!page.records.is_empty() || !page.fragments.is_empty()))
                && let Some(mut reader) = app.session.companion_owned_history_reader(&request.header).map_err(|e| e.to_string())?
            {
                let tx = self.paging.tx.clone();
                self.paging.cancel = Arc::new(AtomicBool::new(false));
                let cancel = self.paging.cancel.clone();
                self.paging.alive.store(true, Ordering::Relaxed);
                let guard = WorkerAlive(self.paging.alive.clone());
                std::thread::Builder::new()
                    .name("companion-history-preview".into())
                    .spawn(move || {
                        let _guard = guard;
                        if cancel.load(Ordering::Relaxed) {
                            return;
                        }
                        let preview = historical::read_native(&mut reader, &page, request.preview_cursor);
                        if !cancel.load(Ordering::Relaxed) {
                            let _ = tx.try_send(ReadMessage {
                                retention: reader.into_retention(),
                                request: request.id,
                                scope: request.scope,
                                header: request.header.clone(),
                                transaction: request.header.transaction_id,
                                cursor: request.cursor,
                                result: ReadContent::Preview(preview),
                            });
                        }
                    })
                    .map_err(|e| e.to_string())?;
            } else {
                self.paging.completed = request.id;
            }
        }
        Ok(())
    }
    pub(super) fn begin_history(
        &mut self,
        reader: HistoryReader,
        identity: RoundIdentity,
        tail: Vec<Value>,
        prompt: String,
        settings: Arc<DevelopSettings>,
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
                let mut reader = reader;
                let result = assemble(&mut reader, tail, &cancel, &prompt, &settings);
                if !cancel.load(Ordering::Relaxed) && tx.try_send(WorkerMessage::HistoryPrepared { identity, result }).is_err() {
                    cancel.store(true, Ordering::Relaxed);
                }
            })
            .map_err(|e| e.to_string())?;
        self.workers.insert(photo, alive);
        Ok(())
    }
}
fn assemble(
    reader: &mut HistoryReader,
    tail: Vec<Value>,
    cancel: &AtomicBool,
    prompt: &str,
    settings: &DevelopSettings,
) -> Result<Vec<Value>, String> {
    if !reader.header().history_complete() {
        return Err(projection::invalid());
    }
    let mut projection = projection::HistoryProjection::new(reader.header().photo_id);
    let mut cursor = 0;
    while cursor < reader.header().message_count {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let page = reader.protocol_messages(cursor, 1, 32 * 1024 * 1024).map_err(|e| e.to_string())?;
        for message in &page.records {
            projection.push_cancellable(message, |picture| reader.preview(picture).map_err(|e| e.to_string()), || cancel.load(Ordering::Relaxed))?;
        }
        cursor = page.next_cursor.unwrap_or(reader.header().message_count);
    }
    for message in &tail {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        projection.push_cancellable(message, |picture| reader.preview(picture).map_err(|e| e.to_string()), || cancel.load(Ordering::Relaxed))?;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    let history = projection.finish()?;
    projection::reserve_input(&history, prompt, settings)?;
    Ok(history)
}
