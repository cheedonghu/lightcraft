//! Visible editor buffers are scoped independently of conversation records.
use super::*;
use lightcraft_engine::companion::EditorState;

pub(super) struct Editor {
    scope: PhotoScope,
    pub(super) data: EditorState,
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
            self.ensure_editor(app, photo)?;
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
            Editor { scope: PhotoScope { library_generation: app.session.library_generation(), photo }, data, revision: 0, restore_scroll: true },
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
            app.session.set_companion_editor(photo, editor.data.clone()).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub(super) fn clear_submitted_draft(&mut self, app: &mut LightcraftApp, photo: PhotoId) -> Result<u64, String> {
        self.ensure_editor(app, photo)?;
        let editor = self.editors.get_mut(&photo).ok_or_else(|| tr("The AI draft is unavailable.").to_owned())?;
        editor.revision = editor.revision.saturating_add(1);
        editor.data.draft.clear();
        let revision = editor.revision;
        app.session.set_companion_editor(photo, editor.data.clone()).map_err(|e| e.to_string())?;
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
        let data = editor.data.clone();
        if let Err(error) = app.session.set_companion_editor(photo, data) {
            app.ai.text = error.to_string();
        }
    }
}
