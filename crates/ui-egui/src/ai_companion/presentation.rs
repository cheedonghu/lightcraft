//! Assistant-only chrome and original vector controls.
use super::*;

pub(super) enum Icon {
    Send,
    Stop,
    Settings,
    Collapse,
}

pub(super) fn icon(ui: &mut egui::Ui, icon: Icon, enabled: bool, label: &str) -> egui::Response {
    let action = matches!(icon, Icon::Send | Icon::Stop);
    let size = if action { 32.0 } else { 26.0 };
    let response = ui.add_enabled(enabled, egui::Button::new("").min_size(egui::vec2(size, size)).frame(false));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(response.rect) {
        let center = response.rect.center();
        let fg = if enabled { egui::Color32::WHITE } else { ui.visuals().weak_text_color() };
        let fg = if action { fg } else { ui.visuals().text_color() };
        let fill = if action && enabled {
            egui::Color32::from_rgb(57, 119, 238)
        } else if action || response.hovered() {
            ui.visuals().widgets.inactive.bg_fill
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().circle_filled(center, size * 0.5, fill);
        let stroke = egui::Stroke::new(1.7, fg);
        match icon {
            Icon::Send => {
                ui.painter().line_segment([center + egui::vec2(0.0, 7.0), center - egui::vec2(0.0, 7.0)], stroke);
                ui.painter().line_segment([center + egui::vec2(-5.0, -2.0), center + egui::vec2(0.0, -7.0)], stroke);
                ui.painter().line_segment([center + egui::vec2(5.0, -2.0), center + egui::vec2(0.0, -7.0)], stroke);
            }
            Icon::Stop => {
                ui.painter().rect_filled(egui::Rect::from_center_size(center, egui::vec2(10.0, 10.0)), 2, fg);
            }
            Icon::Settings => {
                for y in [-5.0, 5.0] {
                    ui.painter().line_segment([center + egui::vec2(-7.0, y), center + egui::vec2(7.0, y)], stroke);
                    ui.painter().circle_filled(center + egui::vec2(if y < 0.0 { -2.0 } else { 3.0 }, y), 2.8, fg);
                }
            }
            Icon::Collapse => {
                ui.painter().line_segment([center + egui::vec2(-3.0, -5.0), center + egui::vec2(2.0, 0.0)], stroke);
                ui.painter().line_segment([center + egui::vec2(-3.0, 5.0), center + egui::vec2(2.0, 0.0)], stroke);
            }
        }
    }
    response.on_hover_text(label).on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn header(app: &mut LightcraftApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let icon_reserve = 2.0 * 26.0 + 3.0 * ui.spacing().item_spacing.x;
        let name_width = (ui.available_width() - icon_reserve - 40.0).clamp(56.0, 88.0);
        ui.add_sized([name_width, 22.0], egui::Label::new(egui::RichText::new(tr("AI Assistant")).heading()).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = icon(ui, Icon::Collapse, true, tr("Collapse"));
            composer_widget(ui, "button:aiCollapse", close.rect);
            if close.clicked() {
                let _ = app.run("panel.ai", json!({}));
            }
            let settings = icon(ui, Icon::Settings, true, tr("Assistant settings"));
            composer_widget(ui, "button:aiSettings", settings.rect);
            if settings.clicked() {
                app.ai.settings_open = !app.ai.settings_open;
            }
            if let Some(photo) = app.ai.displayed_photo(app).and_then(|id| app.session.catalog.photo(id)) {
                ui.add(egui::Label::new(egui::RichText::new(format!("· {}", photo.file_name)).weak()).truncate())
                    .on_hover_text(photo.file_name.as_str());
            }
        });
    });
    ui.add_space(6.0);
    ui.separator();
    settings(app, ui.ctx());
}

fn settings(app: &mut LightcraftApp, ctx: &egui::Context) {
    let mut open = app.ai.settings_open;
    let window = egui::Window::new(tr("Assistant settings"))
        .id(egui::Id::new("ai-settings"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(16))
        .default_pos(egui::pos2((ctx.content_rect().right() - 416.0).max(12.0), 80.0))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            settings_section(ui, "AI service", |ui| {
                health::show(app, ui);
                ui.add_space(4.0);
                ui.add(egui::Label::new(egui::RichText::new(tr("AI analysis sends a compressed preview to the cloud model.")).small().weak()).wrap());
            });
            if app.ai.readonly_task.is_none()
                && let Some(photo) = app.session.active().filter(|_| app.session.selection.ids.len() == 1)
            {
                ui.add_space(4.0);
                let can_clear = !app.ai.jobs.contains_key(&photo)
                    && !app.ai.worker_busy(photo)
                    && !app.session.companion_migration_pending()
                    && app.session.companion_archive_protected_reason().is_none()
                    && !app.ai.conversations.get(&photo).and_then(|c| c.history_error.as_ref()).is_some_and(HistoryError::ownership_unknown);
                settings_section(ui, "Conversation management", |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(tr("Manage the AI conversation for the selected photo.")).small().weak()).wrap());
                    ui.add_space(4.0);
                    let clear = ui.add_enabled(
                        can_clear,
                        egui::Button::new(egui::RichText::new(tr("Delete Conversation")).color(ui.visuals().error_fg_color))
                            .min_size(egui::vec2(0.0, 30.0)),
                    );
                    composer_widget(ui, "button:aiClear", clear.rect);
                    if clear.clicked()
                        && let Err(error) = app.run("ai.clear", json!({}))
                    {
                        app.ai.conversations.entry(photo).or_default().notice = error;
                    }
                });
            }
        });
    if let Some(window) = window {
        crate::widgets::register(ctx, "ai:settings", window.response.rect);
    }
    app.ai.settings_open = open;
}

fn settings_section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(8).inner_margin(12).show(ui, |ui| {
        ui.set_width((width - 24.0).max(0.0));
        ui.strong(tr(title));
        body(ui);
    });
}

pub(super) fn welcome(app: &mut LightcraftApp, ui: &mut egui::Ui, photo: PhotoId) {
    ui.add_space(28.0);
    ui.label(egui::RichText::new(tr("Start with your photo")).size(19.0).strong());
    ui.add_space(10.0);
    let intro = ui.label(egui::RichText::new(tr("Understand and naturally optimize your photo. Originals stay on this computer.")).weak());
    composer_widget(ui, "ai:welcome", intro.rect);
    ui.add_space(20.0);
    ui.label(egui::RichText::new(tr("Choose a prompt below, or write your own request.")).small().weak());
    ui.add_space(8.0);
    for (id, label, prompt) in [
        ("button:aiReviewPrompt", "Review this photo", "Please review this photo."),
        ("button:aiOptimizePrompt", "Naturally optimize", "Please naturally optimize this photo."),
    ] {
        let response =
            ui.add_enabled(!app.ai.jobs.contains_key(&photo), egui::Button::new(tr(label)).corner_radius(9).min_size(egui::vec2(0.0, 30.0)));
        composer_widget(ui, id, response.rect);
        if !ui.is_sizing_pass() && response.clicked() {
            app.ai.prompt = tr(prompt).into();
            drafts::capture(app);
        }
        ui.add_space(4.0);
    }
}

pub(super) fn empty(conversation: Option<&Conversation>) -> bool {
    conversation.is_none_or(|c| {
        c.history_error.is_none()
            && c.messages.is_empty()
            && c.pending.is_empty()
            && c.records.is_empty()
            && c.prefix.as_ref().is_none_or(|header| header.message_count == 0)
    })
}
