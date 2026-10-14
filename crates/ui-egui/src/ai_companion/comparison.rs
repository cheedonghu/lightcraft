//! Captured round previews: navigation only, never a render of today's settings.
use super::*;
use lightcraft_engine::companion::storage::{DisplayRecord, DisplayScan, PhotoHeader};

#[derive(Default)]
struct Picture {
    texture: Option<egui::TextureHandle>,
    matches: u64,
}
impl Picture {
    fn status(&self) -> Value {
        self.texture.as_ref().map_or_else(
            || json!({"available":false}),
            |texture| {
                let size = texture.size();
                json!({"available":true,"width":size[0],"height":size[1]})
            },
        )
    }
}
pub(super) struct Comparison {
    scope: PhotoScope,
    round: String,
    header: PhotoHeader,
    selection: lightcraft_engine::Selection,
    binding: Option<card::RoundBinding>,
    scan: Option<DisplayScan>,
    loading: bool,
    before: Picture,
    after: Picture,
}
impl Comparison {
    pub(super) fn status(&self) -> Value {
        json!({"localRoundId":self.round,"photo":self.scope.photo.0,"libraryGeneration":self.scope.library_generation,
            "loading":self.loading,"before":self.before.status(),"after":self.after.status()})
    }
}
pub(super) fn enter(state: &mut State, app: &mut LightcraftApp, params: &Value) -> Result<Value, String> {
    let photo = state.displayed_photo(app).ok_or_else(|| tr("No AI task to view.").to_owned())?;
    let round = params["local_round_id"].as_str().ok_or_else(|| tr("Historical preview unavailable").to_owned())?;
    let nonce = round.strip_prefix("local-").ok_or_else(|| tr("Historical preview unavailable").to_owned())?;
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(tr("Historical preview unavailable").into());
    }
    state.sync_editor(app)?;
    state.load(app, photo)?;
    let conversation = state.conversations.get(&photo).ok_or_else(|| tr("Historical preview unavailable").to_owned())?;
    if conversation.schema_version != 2 || conversation.history_error.is_some() {
        return Err(tr("Historical preview unavailable").into());
    }
    let header = conversation.prefix.clone().ok_or_else(|| tr("Historical preview unavailable").to_owned())?;
    let count = if header.record_count > 0 { header.record_count } else { header.round_count };
    let index = count.checked_sub(1).ok_or_else(|| tr("Historical preview unavailable").to_owned())?;
    state.comparison = Some(Comparison {
        scope: PhotoScope { photo, library_generation: app.session.library_generation() },
        round: round.into(),
        selection: app.session.selection.clone(),
        scan: Some(DisplayScan::new(header.clone(), DisplayRecord::Round(index))),
        header,
        binding: None,
        loading: true,
        before: Picture::default(),
        after: Picture::default(),
    });
    Ok(json!({"opened":true,"photo":photo.0}))
}
fn picture(app: &mut LightcraftApp, ctx: &egui::Context, photo: PhotoId, message: &Value) -> Option<egui::TextureHandle> {
    let images = message["images"].as_array()?;
    if images.len() != 1 || images.first()?["mime_type"] != "image/jpeg" {
        return None;
    }
    let image = images.first()?;
    let bytes = app.session.read_companion_historical_preview(photo, image).ok()?;
    lightcraft_engine::companion::validate_history_image("image/jpeg", &bytes).ok()?;
    let decoded = lightcraft_codecs::decode(&bytes, lightcraft_codecs::DecodeOptions { max_size: Some((640, 640)), max_pixels: 640 * 640 }).ok()?;
    let decoded = decoded.to_srgb8().into_oriented(lightcraft_geom::Orientation::from_exif(decoded.orientation));
    let image = egui::ColorImage::from_rgba_unmultiplied([decoded.width, decoded.height], &decoded.as_bytes());
    Some(ctx.load_texture("ai-round-comparison", image, egui::TextureOptions::LINEAR))
}
pub(super) fn poll(state: &mut State, app: &mut LightcraftApp, ctx: &egui::Context) {
    let Some(mut view) = state.comparison.take() else { return };
    if view.scope.library_generation != app.session.library_generation() || view.selection != app.session.selection || state.exiting {
        return;
    }
    if let Some(mut scan) = view.scan.take() {
        match app.session.companion_display_step(&mut scan) {
            Ok(None) => view.scan = Some(scan),
            Err(_) => view.loading = false,
            Ok(Some(record)) => match scan.target() {
                DisplayRecord::Round(index) => {
                    if record["local_round_id"] == view.round && record["photo_id"].as_u64() == Some(view.scope.photo.0) {
                        view.binding = card::from_record(&record, None).and_then(|card| card.binding);
                        if view.binding.is_some() && view.header.message_count > 0 {
                            view.scan = Some(DisplayScan::new(view.header.clone(), DisplayRecord::Message(0)));
                        } else {
                            view.loading = false;
                        }
                    } else if let Some(index) = index.checked_sub(1) {
                        view.scan = Some(DisplayScan::new(view.header.clone(), DisplayRecord::Round(index)));
                    } else {
                        view.loading = false;
                    }
                }
                DisplayRecord::Message(index) => {
                    if let Some(binding) = &view.binding
                        && record["local_round_id"] == view.round
                        && record["complete_state"] == "complete"
                        && record["role"] == "user"
                    {
                        let slot = if record["local_message_id"] == binding.before_preview_message_id && record["display_kind"] == "request" {
                            Some(&mut view.before)
                        } else if record["local_message_id"] == binding.after_preview_message_id && record["display_kind"] == "execution_feedback" {
                            Some(&mut view.after)
                        } else {
                            None
                        };
                        if let Some(slot) = slot {
                            slot.matches = slot.matches.saturating_add(1);
                            slot.texture = if slot.matches == 1 { picture(app, ctx, view.scope.photo, &record) } else { None };
                        }
                    }
                    let next = index.saturating_add(1);
                    if next < view.header.message_count {
                        view.scan = Some(DisplayScan::new(view.header.clone(), DisplayRecord::Message(next)));
                    } else {
                        view.loading = false;
                    }
                }
                DisplayRecord::LegacyPlan(_) => view.loading = false,
            },
        }
    }
    if view.loading {
        ctx.request_repaint();
    }
    state.comparison = Some(view);
}
pub(super) fn return_button(app: &mut LightcraftApp, ui: &mut egui::Ui) {
    let button = ui.button(tr("Return to Current Photo"));
    composer_widget(ui, "button:aiReturnCurrent", button.rect);
    if !ui.is_sizing_pass() && button.clicked() {
        let _ = app.run("ai.returnCurrent", json!({}));
    }
}
fn tile(ui: &mut egui::Ui, rect: egui::Rect, picture: &Picture, label: &str, id: &str) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.label(tr(label));
    let area = child.available_rect_before_wrap();
    if let Some(texture) = &picture.texture {
        let size = texture.size_vec2();
        let factor = (area.width() / size.x).min(area.height() / size.y).max(0.0);
        let image = egui::Rect::from_center_size(area.center(), size * factor);
        child.painter().image(texture.id(), image, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        composer_widget(&child, id, image);
    } else {
        child.label(tr("Historical preview unavailable"));
    }
}
pub(super) fn show(app: &mut LightcraftApp, ui: &mut egui::Ui) -> bool {
    if app.ai.comparison.is_none() {
        return false;
    }
    ui.label(tr("Historical preview"));
    return_button(app, ui);
    let Some(view) = app.ai.comparison.as_ref() else { return true };
    if view.loading {
        ui.label(tr("Loading historical preview…"));
    }
    let area = ui.available_rect_before_wrap();
    let (before, after) = if area.width() >= 600.0 {
        let middle = area.center().x;
        (
            egui::Rect::from_min_max(area.min, egui::pos2(middle - 8.0, area.bottom())),
            egui::Rect::from_min_max(egui::pos2(middle + 8.0, area.top()), area.max),
        )
    } else {
        let middle = area.center().y;
        (
            egui::Rect::from_min_max(area.min, egui::pos2(area.right(), middle - 8.0)),
            egui::Rect::from_min_max(egui::pos2(area.left(), middle + 8.0), area.max),
        )
    };
    tile(ui, before, &view.before, "Before this round", "image:aiCompareBefore");
    tile(ui, after, &view.after, "After this round", "image:aiCompareAfter");
    true
}
