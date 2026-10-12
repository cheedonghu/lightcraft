//! Normal catalog persistence and bounded attachment hydration.
use super::*;
impl State {
    pub(super) fn load(&mut self, app: &LightcraftApp, photo: PhotoId) -> Result<(), String> {
        if self.conversations.contains_key(&photo) {
            return Ok(());
        }
        let mut conversation = Conversation::default();
        if let Some(archive) = app.session.photo_companion(photo).map_err(|e| e.to_string())? {
            if archive.get("schema_version").and_then(Value::as_u64) != Some(1) {
                return Err("AI 对话格式不支持，未删减历史。".into());
            }
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
            return Ok(());
        };
        if let Some(library) = app.session.library.as_mut() {
            for message in &mut conversation.messages {
                if let Some(images) = message.get_mut("images").and_then(Value::as_array_mut) {
                    for image in images {
                        if let Some(data) = image.get("data").and_then(Value::as_str) {
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
        let checkpoint = conversation.checkpoint.as_ref().map(|c| json!({"original":c.original,"expected":c.expected,"eligible":c.eligible}));
        app.session
            .set_photo_companion(
                photo,
                Some(json!({"schema_version":1,"messages":conversation.messages,"rounds":conversation.rounds,"checkpoint":checkpoint})),
            )
            .map_err(|e| e.to_string())?;
        // Normal journal persistence, independent of the already visible execution result.
        app.session.persist_if_dirty();
        Ok(())
    }
    pub(super) fn history(&mut self, app: &mut LightcraftApp, photo: PhotoId) -> Result<Vec<Value>, String> {
        self.load(app, photo)?;
        let mut messages = self.conversations.get(&photo).ok_or("AI 会话不存在。")?.messages.clone();
        if messages.len() > 1024 {
            return Err("AI 完整历史超过服务限制，未删减历史。".into());
        }
        let mut images = 0usize;
        let mut text = 0usize;
        for message in &mut messages {
            text = text.saturating_add(message.get("text").and_then(Value::as_str).map(str::len).unwrap_or(0));
            if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                text = text.saturating_add(serde_json::to_vec(calls).map_err(|_| "AI 工具历史无效。")?.len());
            }
            if let Some(pictures) = message.get_mut("images").and_then(Value::as_array_mut) {
                images = images.saturating_add(pictures.len());
                if images > 256 {
                    return Err("AI 完整历史超过服务限制，未删减历史。".into());
                }
                for picture in pictures {
                    if let Some(name) = picture.get("attachment").and_then(Value::as_str) {
                        let data = app
                            .session
                            .library
                            .as_mut()
                            .ok_or("AI 预览附件的图库已关闭。")?
                            .read_companion_attachment(name)
                            .map_err(|e| e.to_string())?;
                        picture.as_object_mut().ok_or("AI 图片记录无效。")?.remove("attachment");
                        picture["data"] = json!(Base64::encode_string(&data));
                    }
                }
            }
        }
        if images > 256 || text > 256 * 1024 || serde_json::to_vec(&messages).map_err(|_| "AI 历史无效。")?.len() > 32 * 1024 * 1024 {
            return Err("AI 完整历史超过服务限制，未删减历史。".into());
        }
        Ok(messages)
    }
}
