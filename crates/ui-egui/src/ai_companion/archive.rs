//! Normal catalog persistence and bounded attachment hydration.
use super::*;
impl State {
    pub(super) fn load(&mut self, app: &mut LightcraftApp, photo: PhotoId) -> Result<(), String> {
        let pending = app.session.companion_migration_pending();
        if self.conversations.get(&photo).is_some_and(|c| c.legacy_readonly && c.prefix.is_none()) {
            // A pre-stage source view is only a bounded compatibility cache; once
            // a graph/activation is ready, reload it rather than freezing an empty view.
            self.conversations.remove(&photo);
        }
        if !pending && let Some(conversation) = self.conversations.get_mut(&photo) {
            conversation.legacy_readonly = false;
        }
        if let Some(conversation) = self.conversations.get(&photo) {
            let prefix = conversation.prefix.clone();
            let loaded = conversation.view.as_ref().map(|view| view.cursor);
            if let Some(prefix) = prefix {
                let cursor = self.initial_page(app, photo, &prefix);
                if loaded != Some(cursor) && !self.paging.has_request(photo, &prefix.transaction_id) {
                    self.request_page(app, photo, cursor)?;
                }
            }
            return Ok(());
        }
        let _ = app.session.prepare_companion_revert(photo);
        let mut conversation = Conversation {
            scope: Some(PhotoScope { library_generation: app.session.library_generation(), photo }),
            legacy_readonly: pending,
            ..Default::default()
        };
        let header = if app.session.library.is_some() {
            match app.session.companion_photo_header(photo).map_err(|e| e.to_string())? {
                Some(header) => Some(header),
                None if pending => app.session.companion_legacy_photo_header(photo).map_err(|e| e.to_string())?,
                None => None,
            }
        } else {
            None
        };
        let archive = if let Some(header) = header {
            let metadata = app.session.companion_metadata_at(&header).map_err(|e| e.to_string())?;
            conversation.prefix = Some(header);
            Some(metadata)
        } else if pending {
            // The owning migration producer will publish a bounded completed
            // header. Until then retain the independent editor and wait; never
            // scan/materialize the original v1 conversation on a frame.
            None
        } else {
            app.session.load_companion_conversation(photo).map_err(|e| e.to_string())?
        };
        if let Some(archive) = archive {
            if !matches!(archive.get("schema_version").and_then(Value::as_u64), Some(1 | 2)) {
                return Err("AI 对话格式不支持，未删减历史。".into());
            }
            conversation.schema_version = archive["schema_version"].as_u64().unwrap_or(1);
            conversation.records = archive.get("round_records").and_then(Value::as_array).cloned().unwrap_or_default();
            conversation.messages = if conversation.prefix.is_some() {
                Vec::new()
            } else {
                archive.get("messages").and_then(Value::as_array).ok_or("AI 对话记录无效。")?.clone()
            };
            conversation.rounds = archive.get("rounds").and_then(Value::as_array).cloned().unwrap_or_default();
            // A disk record is historical display data. Runtime publication or a
            // separately verified durable certificate must grant revert authority;
            // never replace a saved version/source with the current photo's identity.
        }
        app.session.pin_companion_prefix(photo, conversation.prefix.clone()).map_err(|e| e.to_string())?;
        self.conversations.insert(photo, conversation);
        self.prune_cache(app, photo);
        if let Some(prefix) = self.conversations.get(&photo).and_then(|c| c.prefix.clone()) {
            let cursor = self.initial_page(app, photo, &prefix);
            self.request_page(app, photo, cursor)?;
        }
        Ok(())
    }
    pub(super) fn save(&mut self, app: &mut LightcraftApp, photo: PhotoId) -> Result<(), String> {
        self.stage_editors(app)?;
        let Some(conversation) = self.conversations.get_mut(&photo) else {
            self.pending_local_saves.remove(&photo);
            return Ok(());
        };
        if conversation.scope.is_some_and(|scope| scope != PhotoScope { library_generation: app.session.library_generation(), photo }) {
            return Err("AI 会话属于已关闭的图库，未写入当前图库。".into());
        }
        if let Some(checkpoint) = conversation.checkpoint.as_mut()
            && checkpoint.eligible
            && !current(app, checkpoint)
        {
            checkpoint.eligible = false;
        }
        // Archive a copy: a failed attachment/write must retain the complete in-memory result.
        for (sequence, message) in conversation.messages.iter_mut().enumerate() {
            if message.get("local_message_id").is_none()
                && let Some(round) = message.get("local_round_id").and_then(Value::as_str)
            {
                message["local_message_id"] = json!(format!("{round}-message-{sequence}"));
            }
        }
        let mut messages = conversation.messages.clone();
        if let Some(library) = app.session.library.as_mut() {
            for message in &mut messages {
                if let Some(images) = message.get_mut("images").and_then(Value::as_array_mut) {
                    for image in images {
                        if conversation.schema_version == 1
                            && let Some(data) = image.get("data").and_then(Value::as_str)
                        {
                            if data.len() > 176 * 1024 {
                                return Err("AI 预览附件过大。".into());
                            }
                            let data = Base64::decode_vec(data).map_err(|_| "AI 预览附件无效。")?;
                            let name = library.companion_attachment(photo, &data).map_err(|e| e.to_string())?;
                            image.as_object_mut().ok_or("AI 图片记录无效。")?.remove("data");
                            image["attachment"] = json!(name);
                        }
                    }
                }
            }
        }
        let checkpoint = conversation
            .checkpoint
            .as_ref()
            .map(|c| json!({"original":c.original,"expected":c.expected,"eligible":c.eligible,"local_round_id":c.local_round_id}));
        if conversation.schema_version == 2 && app.session.library.is_some() {
            conversation.dirty = true;
            let result = app.session.save_companion_tail(
                photo,
                conversation.prefix.as_ref(),
                &messages,
                &conversation.rounds,
                &conversation.records,
                &json!({"schema_version":2,"checkpoint":checkpoint}),
            );
            if let Err(error) = result {
                conversation.dirty_queued = app.session.companion_tail_unsaved(photo);
                return Err(error.to_string());
            }
            // Only the caller-owned current round is retained/re-normalized. Never
            // reload the complete saved conversation after a successful write.
            let header = app.session.companion_photo_header(photo).map_err(|e| e.to_string())?.ok_or("AI 会话缺失。")?;
            let start = conversation.prefix.as_ref().map(|h| h.message_count).unwrap_or(0);
            let mut normalized = Vec::new();
            let mut cursor = start;
            while cursor < header.message_count {
                let result = app.session.companion_messages_bounded(&header, cursor, 128, 32 * 1024 * 1024).map_err(|e| e.to_string())?;
                normalized.extend(result.records);
                cursor = result.next_cursor.unwrap_or(header.message_count);
            }
            conversation.messages = normalized;
            conversation.dirty = false;
            conversation.dirty_queued = false;
        } else {
            app.session.save_companion_conversation(photo,
                json!({"schema_version":conversation.schema_version,"messages":messages,"rounds":conversation.rounds,"round_records":conversation.records,"checkpoint":checkpoint})
            ).map_err(|e| e.to_string())?;
        }
        // Normal journal persistence, independent of the already visible execution result.
        app.session.persist_if_dirty();
        self.pending_local_saves.remove(&photo);
        Ok(())
    }
    pub(super) fn history(&mut self, app: &mut LightcraftApp, photo: PhotoId) -> Result<Vec<Value>, String> {
        self.load(app, photo)?;
        let conversation = self.conversations.get(&photo).ok_or("AI 会话不存在。")?;
        let tail = conversation.messages.clone();
        let prefix = conversation.prefix.clone();
        if prefix.as_ref().is_some_and(|h| !h.history_complete()) {
            return Err(projection::invalid());
        }
        let mut projection = projection::HistoryProjection::new(photo);
        if let Some(prefix) = prefix {
            let mut cursor = 0;
            while cursor < prefix.message_count {
                let page = app.session.companion_protocol_messages_bounded(&prefix, cursor, 1, 32 * 1024 * 1024).map_err(|e| e.to_string())?;
                for message in &page.records {
                    projection.push(message, |picture| {
                        let name = picture.get("attachment").and_then(Value::as_str).ok_or("AI 预览附件无效。")?;
                        let library = app.session.library.as_mut().ok_or("AI 预览附件的图库已关闭。")?;
                        if name.starts_with("companion/v2/") {
                            library.read_companion_preview(picture)
                        } else {
                            library.read_companion_attachment(name)
                        }
                        .map_err(|e| e.to_string())
                    })?;
                }
                cursor = page.next_cursor.unwrap_or(prefix.message_count);
            }
        }
        for message in &tail {
            projection.push(message, |picture| {
                let name = picture.get("attachment").and_then(Value::as_str).ok_or("AI 预览附件无效。")?;
                let library = app.session.library.as_mut().ok_or("AI 预览附件的图库已关闭。")?;
                if name.starts_with("companion/v2/") { library.read_companion_preview(picture) } else { library.read_companion_attachment(name) }
                    .map_err(|e| e.to_string())
            })?;
        }
        projection.finish()
    }
    pub(super) fn read_page(
        &mut self,
        app: &mut LightcraftApp,
        photo: PhotoId,
        cursor: lightcraft_engine::companion::storage::HistoryCursor,
    ) -> Result<(), String> {
        let Some(prefix) = self.conversations.get(&photo).and_then(|c| c.prefix.clone()) else { return Ok(()) };
        // Capture before the first fallback read, and keep it through all
        // owner/round scans and inactive pauses until the actual view drops.
        let retention = app.session.retain_companion_history(&prefix).map_err(|e| e.to_string())?;
        let page = app.session.companion_history_slice_at(&prefix, cursor, 256 * 1024).map_err(|e| e.to_string())?;
        let cached = self
            .conversations
            .get(&photo)
            .and_then(|c| c.view.as_ref())
            .filter(|v| v.captured_header == prefix)
            .map(|v| v.fragment_messages.clone())
            .unwrap_or_default();
        let mut wanted: BTreeSet<_> = page.records.iter().filter_map(|m| m["local_round_id"].as_str()).map(str::to_owned).collect();
        let mut presentation = BTreeMap::new();
        let owners: VecDeque<_> = page
            .items
            .iter()
            .filter_map(|item| match item {
                lightcraft_engine::companion::storage::HistoryItem::Fragment { record, .. } => {
                    if let Some(message) = cached.get(record) {
                        if let Some(id) = message["local_round_id"].as_str() {
                            wanted.insert(id.to_owned());
                        }
                        presentation.insert(*record, message.clone());
                        None
                    } else {
                        Some(*record)
                    }
                }
                _ => None,
            })
            .collect();
        let total = if prefix.record_count > 0 { prefix.record_count } else { prefix.round_count };
        let conversation = self.conversations.get_mut(&photo).ok_or("AI 会话不存在。")?;
        conversation.view = Some(HistoryView {
            captured_header: prefix.clone(),
            _retention: retention,
            items: page.items,
            cursor,
            next: page.next,
            messages: page.records,
            fragments: page.fragments,
            fragment_messages: presentation.clone(),
            bytes: page.bytes,
            cards: Vec::new(),
            diagnostic: None,
            legacy_plan: None,
            legacy_scan: (prefix.legacy_plan_count() > 0).then(|| {
                lightcraft_engine::companion::storage::DisplayScan::new(
                    prefix.clone(),
                    lightcraft_engine::companion::storage::DisplayRecord::LegacyPlan(conversation.legacy_cursor),
                )
            }),
            preview: None,
            cards_bytes: presentation
                .values()
                .map(|v| lightcraft_engine::companion::storage::record_size(v).unwrap_or(usize::MAX))
                .fold(0usize, usize::saturating_add),
            card_cursor: (!owners.is_empty() || (total > 0 && !wanted.is_empty())).then_some(0),
            card_pending: wanted,
            card_owners: owners,
            card_scan: None,
        });
        self.accept_page(app, photo, cursor);
        self.prune_cache(app, photo);
        Ok(())
    }
    pub(super) fn prune_cache(&mut self, app: &mut LightcraftApp, keep: PhotoId) {
        // Dirty results remain owned until successfully saved; active work is
        // separately service-budgeted. Evict only saved inactive read caches.
        loop {
            let bytes: usize = self
                .conversations
                .values()
                .map(|c| {
                    c.view.as_ref().map(|v| v.bytes.saturating_add(v.cards_bytes)).unwrap_or(0).saturating_add(
                        c.messages
                            .iter()
                            .chain(c.records.iter())
                            .chain(c.rounds.iter())
                            .map(|v| lightcraft_engine::companion::storage::record_size(v).unwrap_or(usize::MAX))
                            .fold(0usize, usize::saturating_add),
                    )
                })
                .fold(0usize, usize::saturating_add);
            if self.conversations.len() <= 16 && bytes <= 2 * 1024 * 1024 {
                break;
            }
            let candidate = self.conversations.iter().find(|(p, c)| **p != keep && !c.dirty && !self.jobs.contains_key(p)).map(|(p, _)| *p);
            let Some(candidate) = candidate else { break };
            self.conversations.remove(&candidate);
            let _ = app.session.pin_companion_prefix(candidate, None);
        }
    }
}
