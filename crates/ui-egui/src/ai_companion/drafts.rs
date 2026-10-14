//! Visible editor buffers are scoped independently of conversation records.
use super::*;
use lightcraft_engine::companion::EditorState;

pub(super) struct Editor {
    scope: PhotoScope,
    pub(super) data: EditorState,
    known_draft: bool,
    revision: u64,
    pub(super) restore_scroll: bool,
}
impl State {
    pub(super) fn sync_editor(&mut self, app: &mut LightcraftApp) -> Result<(), String> {
        self.capture_draft(app)?;
        let active = app.session.active().filter(|_| app.session.selection.ids.len() == 1);
        if self.editor_photo != active {
            self.editor_photo = active;
            self.prompt.clear();
            if let Some(photo) = active {
                self.ensure_editor(app, photo)?;
                if let Some(editor) = self.editors.get_mut(&photo) {
                    self.prompt = editor.data.draft.clone();
                    editor.restore_scroll = true;
                }
            }
        } else if let Some(photo) = active {
            let missing = !self.editors.contains_key(&photo);
            self.ensure_editor(app, photo)?;
            if missing && let Some(editor) = self.editors.get_mut(&photo) {
                self.prompt = editor.data.draft.clone();
                editor.restore_scroll = true;
            }
        }
        // Saved inactive editor buffers can be reloaded independently; never evict unsaved drafts.
        let dirty: BTreeSet<_> = app.session.companion_dirty_editors().into_iter().collect();
        let mut bytes = self.editors.values().fold(0usize, |bytes, editor| bytes.saturating_add(editor.data.draft.capacity()));
        if self.editors.len() > 16 || bytes > 2 * 1024 * 1024 {
            let saved: Vec<_> = self
                .editors
                .keys()
                .copied()
                .filter(|photo| {
                    Some(*photo) != self.editor_photo
                        && !self.jobs.contains_key(photo)
                        && !self.dirty_editors.contains(photo)
                        && !dirty.contains(photo)
                })
                .collect();
            for photo in saved {
                if self.editors.len() <= 16 && bytes <= 2 * 1024 * 1024 {
                    break;
                }
                if let Some(editor) = self.editors.remove(&photo) {
                    bytes = bytes.saturating_sub(editor.data.draft.capacity());
                }
            }
        }
        Ok(())
    }
    pub(super) fn sync_editor_deferred(&mut self, app: &mut LightcraftApp) -> Result<(), String> {
        let captured = self.capture_draft(app);
        let active = app.session.active().filter(|_| app.session.selection.ids.len() == 1);
        if self.editor_photo != active {
            self.editor_photo = active;
            self.prompt = active.and_then(|photo| self.editors.get(&photo)).map(|editor| editor.data.draft.clone()).unwrap_or_default();
            if let Some(editor) = active.and_then(|photo| self.editors.get_mut(&photo)) {
                editor.restore_scroll = true;
            }
        }
        captured
    }
    fn ensure_editor(&mut self, app: &mut LightcraftApp, photo: PhotoId) -> Result<(), String> {
        if self.editors.contains_key(&photo) {
            return Ok(());
        }
        let loaded = app.session.companion_editor(photo).map_err(|e| e.to_string());
        let data = loaded.as_ref().cloned().unwrap_or_default();
        // Keep a writable runtime overlay even when the archive is protected. It
        // cannot overwrite the unreadable source; normal saves report it unsaved.
        self.editors.insert(
            photo,
            Editor {
                scope: PhotoScope { library_generation: app.session.library_generation(), photo },
                data,
                known_draft: loaded.is_ok(),
                revision: 0,
                restore_scroll: true,
            },
        );
        loaded.map(|_| ())
    }
    pub(super) fn capture_draft(&mut self, app: &mut LightcraftApp) -> Result<(), String> {
        let Some(photo) = self.editor_photo else { return Ok(()) };
        let Some(editor) = self.editors.get_mut(&photo) else { return Ok(()) };
        if editor.scope.library_generation != app.session.library_generation() {
            return Ok(());
        }
        if editor.data.draft != self.prompt {
            editor.revision = editor.revision.saturating_add(1);
            editor.data.draft.clone_from(&self.prompt);
            editor.known_draft = true;
            app.session.set_companion_editor(photo, editor.data.clone()).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub(super) fn clear_submitted_draft(&mut self, app: &mut LightcraftApp, photo: PhotoId) -> Result<u64, String> {
        self.ensure_editor(app, photo)?;
        let editor = self.editors.get_mut(&photo).ok_or_else(|| tr("The AI draft is unavailable.").to_owned())?;
        editor.revision = editor.revision.saturating_add(1);
        let changed = !editor.data.draft.is_empty();
        editor.data.draft.clear();
        let revision = editor.revision;
        if changed {
            app.session.set_companion_editor(photo, editor.data.clone()).map_err(|e| e.to_string())?;
        }
        if self.editor_photo == Some(photo) {
            self.prompt.clear();
        }
        Ok(revision)
    }
    pub(super) fn restore_request(&mut self, photo: PhotoId, job: &Job) {
        // A later edit wins. Delayed terminal receipts cannot replace it, even on the same photo.
        self.restore_text(photo, job.identity.scope, job.draft_revision, &job.prompt);
    }
    pub(super) fn restore_text(&mut self, photo: PhotoId, scope: PhotoScope, revision: u64, prompt: &str) {
        let Some(editor) = self.editors.get_mut(&photo) else { return };
        if editor.scope != scope || editor.revision != revision || !editor.data.draft.is_empty() {
            return;
        }
        editor.revision = editor.revision.saturating_add(1);
        editor.data.draft = prompt.into();
        editor.known_draft = true;
        if self.editor_photo == Some(photo) {
            self.prompt = prompt.into();
        }
        self.dirty_editors.insert(photo);
    }
    pub(super) fn stage_editors(&mut self, app: &mut LightcraftApp) -> Result<(), String> {
        self.capture_draft(app)?;
        for photo in self.dirty_editors.iter().copied().collect::<Vec<_>>() {
            if let Some(editor) = self.editors.get(&photo) {
                if editor.scope.library_generation != app.session.library_generation() {
                    return Err(tr("The AI draft belongs to a closed library.").into());
                }
                app.session.set_companion_editor(photo, editor.data.clone()).map_err(|e| e.to_string())?;
            }
            self.dirty_editors.remove(&photo);
        }
        Ok(())
    }
}

pub(super) fn capture(app: &mut LightcraftApp) {
    let mut state = std::mem::take(&mut app.ai);
    if let Err(error) = state.stage_editors(app) {
        state.text = error;
    }
    app.ai = state;
}

pub(super) fn scroll_position(app: &mut LightcraftApp, photo: PhotoId, offset: f32, tail: bool) {
    if !offset.is_finite() || offset < 0.0 {
        return;
    }
    let Some(editor) = app.ai.editors.get_mut(&photo) else { return };
    if (editor.data.scroll_offset - offset).abs() > 0.5 || editor.data.follow_tail != tail {
        editor.data.scroll_offset = offset;
        editor.data.follow_tail = tail;
        // A failed load's empty overlay is not a replacement authored by the user.
        if editor.known_draft {
            let data = editor.data.clone();
            if let Err(error) = app.session.set_companion_editor(photo, data) {
                app.ai.text = error.to_string();
            }
        }
    }
}

impl State {
    pub(super) fn initial_page(
        &mut self,
        app: &mut LightcraftApp,
        photo: PhotoId,
        header: &lightcraft_engine::companion::storage::PhotoHeader,
    ) -> lightcraft_engine::companion::storage::HistoryCursor {
        if let Err(error) = self.ensure_editor(app, photo) {
            self.text = error;
        }
        self.editors
            .get(&photo)
            .filter(|e| {
                e.data.reading_epoch == Some(header.conversation_epoch)
                    && !(e.data.follow_tail && e.data.reading_count.is_some_and(|count| count != header.message_count))
            })
            .and_then(|e| e.data.reading_cursor)
            .filter(|cursor| cursor.record < header.message_count)
            .unwrap_or(lightcraft_engine::companion::storage::HistoryCursor { record: header.message_count.saturating_sub(32), offset: 0 })
    }
    pub(super) fn navigate_page(
        &mut self,
        app: &mut LightcraftApp,
        photo: PhotoId,
        cursor: lightcraft_engine::companion::storage::HistoryCursor,
    ) -> Result<(), String> {
        self.bottom_reading.remove(&photo);
        if let Some(conversation) = self.conversations.get_mut(&photo)
            && conversation.view.as_ref().is_none_or(|view| view.cursor != cursor)
        {
            conversation.preview_cursor = 0;
        }
        self.ensure_editor(app, photo)?;
        let prefix = self.conversations.get(&photo).and_then(|c| c.prefix.as_ref());
        let epoch = prefix.map(|h| h.conversation_epoch);
        let count = prefix.map(|h| h.message_count);
        if let Some(editor) = self.editors.get_mut(&photo) {
            editor.data.reading_cursor = Some(cursor);
            editor.data.reading_epoch = epoch;
            editor.data.reading_count = count;
            editor.data.follow_tail = false;
            editor.data.scroll_offset = 0.0;
            editor.restore_scroll = true;
            if editor.known_draft {
                app.session.set_companion_editor(photo, editor.data.clone()).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    pub(super) fn accept_page(&mut self, app: &mut LightcraftApp, photo: PhotoId, cursor: lightcraft_engine::companion::storage::HistoryCursor) {
        if let Err(error) = self.ensure_editor(app, photo) {
            self.text = error;
        }
        let prefix = self.conversations.get(&photo).and_then(|c| c.prefix.as_ref());
        let epoch = prefix.map(|h| h.conversation_epoch);
        let count = prefix.map(|h| h.message_count);
        if let Some(editor) = self.editors.get_mut(&photo) {
            editor.data.reading_cursor = Some(cursor);
            editor.data.reading_epoch = epoch;
            editor.data.reading_count = count;
            editor.restore_scroll = true;
            if editor.known_draft
                && let Err(error) = app.session.set_companion_editor(photo, editor.data.clone())
            {
                self.text = error.to_string();
            }
        }
    }
}

impl State {
    pub(super) fn ensure_bottom(&mut self, app: &mut LightcraftApp, photo: PhotoId) -> Result<(), String> {
        self.ensure_editor(app, photo)?;
        let header = self.conversations.get(&photo).and_then(|c| c.prefix.clone());
        if let Some(header) = header {
            let cursor = lightcraft_engine::companion::storage::HistoryCursor { record: header.message_count.saturating_sub(1), offset: 0 };
            self.navigate_page(app, photo, cursor)?;
            self.bottom_reading.insert(photo);
            self.request_page(app, photo, cursor)?;
        }
        if let Some(editor) = self.editors.get_mut(&photo) {
            editor.data.follow_tail = true;
            editor.restore_scroll = true;
            if editor.known_draft {
                app.session.set_companion_editor(photo, editor.data.clone()).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}
