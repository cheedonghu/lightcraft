//! Normal catalog persistence and bounded attachment hydration.
use super::*;
impl State {
    pub(super) fn load(&mut self, app: &mut LightcraftApp, photo: PhotoId) -> Result<(), String> {
        if self.conversations.contains_key(&photo) {
            return Ok(());
        }
        let mut conversation =
            Conversation { scope: Some(PhotoScope { library_generation: app.session.library_generation(), photo }), ..Default::default() };
        if let Some(archive) = app.session.load_companion_conversation(photo).map_err(|e| e.to_string())? {
            if !matches!(archive.get("schema_version").and_then(Value::as_u64), Some(1 | 2)) {
                return Err("AI 对话格式不支持，未删减历史。".into());
            }
            conversation.schema_version = archive["schema_version"].as_u64().unwrap_or(1);
            conversation.records = archive.get("round_records").and_then(Value::as_array).cloned().unwrap_or_default();
            conversation.messages = archive.get("messages").and_then(Value::as_array).ok_or("AI 对话记录无效。")?.clone();
            conversation.rounds = archive.get("rounds").and_then(Value::as_array).cloned().unwrap_or_default();
            if let Some(saved) = archive.get("checkpoint").filter(|v| !v.is_null()) {
                let original: DevelopSettings =
                    serde_json::from_value(saved.get("original").cloned().ok_or("AI 撤回起点缺失。")?).map_err(|_| "AI 撤回起点无效。")?;
                let expected: DevelopSettings =
                    serde_json::from_value(saved.get("expected").cloned().ok_or("AI 编辑结果缺失。")?).map_err(|_| "AI 编辑结果无效。")?;
                let p = app.session.catalog.photo(photo).ok_or("照片不存在。")?;
                conversation.checkpoint = Some(Checkpoint {
                    photo,
                    generation: app.session.library_generation(),
                    original: Arc::new(original),
                    expected: Arc::new(expected),
                    revision: app.session.photo_revision(photo),
                    content: lightcraft_engine::media::content_key(p),
                    eligible: saved.get("eligible").and_then(Value::as_bool) == Some(true),
                });
            }
        }
        self.conversations.insert(photo, conversation);
        Ok(())
    }
    pub(super) fn save(&mut self, app: &mut LightcraftApp, photo: PhotoId) -> Result<(), String> {
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
        let checkpoint = conversation.checkpoint.as_ref().map(|c| json!({"original":c.original,"expected":c.expected,"eligible":c.eligible,"local_round_id":conversation.records.iter().rev().find(|r|r.get("recent_complete_result").is_some()).and_then(|r|r.get("local_round_id"))}));
        app.session
            .save_companion_conversation(
                photo,
                json!({"schema_version":conversation.schema_version,"messages":messages,"rounds":conversation.rounds,"round_records":conversation.records,"checkpoint":checkpoint}),
            )
            .map_err(|e| e.to_string())?;
        if conversation.schema_version == 2
            && app.session.library.is_some()
            && let Some(saved) = app.session.load_companion_conversation(photo).map_err(|e| e.to_string())?
        {
            conversation.messages = saved.get("messages").and_then(Value::as_array).cloned().ok_or("AI 对话记录无效。")?;
        }
        // Normal journal persistence, independent of the already visible execution result.
        app.session.persist_if_dirty();
        self.pending_local_saves.remove(&photo);
        Ok(())
    }
    pub(super) fn history(&mut self, app: &mut LightcraftApp, photo: PhotoId) -> Result<Vec<Value>, String> {
        self.load(app, photo)?;
        let messages = &self.conversations.get(&photo).ok_or("AI 会话不存在。")?.messages;
        let mut projection = projection::HistoryProjection::new(photo);
        for message in messages {
            projection.push(message, |picture| {
                let name = picture.get("attachment").and_then(Value::as_str).ok_or("AI 预览附件无效。")?;
                let library = app.session.library.as_mut().ok_or("AI 预览附件的图库已关闭。")?;
                if name.starts_with("companion/v2/") { library.read_companion_preview(picture) } else { library.read_companion_attachment(name) }
                    .map_err(|e| e.to_string())
            })?;
        }
        projection.finish()
    }
}
