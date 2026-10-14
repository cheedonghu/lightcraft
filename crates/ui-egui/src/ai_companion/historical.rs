//! Verified original references and page-local navigation; pixels use Attachments.
use super::*;
use lightcraft_engine::companion::storage::{HistoryItem, HistoryReader, HistorySlice};
pub(super) struct Preview {
    pub position: u64,
    pub total: u64,
    pub record: u64,
    pub index: u64,
    pub mime: String,
    pub image: Value,
    available: bool,
}
impl Preview {
    pub fn status(&self) -> Value {
        json!({"position":self.position,"total":self.total,"record":self.record,"index":self.index,"mime_type":self.mime,"available":self.available})
    }
    pub fn bytes(&self) -> usize {
        lightcraft_engine::companion::storage::record_size(&self.image).unwrap_or(usize::MAX)
    }
}
pub(super) fn show(ui: &mut egui::Ui, preview: &Preview, attachments: &mut attachments::Attachments) -> Option<u64> {
    let mut next = None;
    ui.group(|ui| {
        ui.label(tr("Original historical preview"));
        ui.horizontal_wrapped(|ui| {
            let previous = ui.add_enabled(preview.position > 0, egui::Button::new(tr("Previous historical image")));
            composer_widget(ui, "button:aiHistoricalPrevious", previous.rect);
            if !ui.is_sizing_pass() && previous.clicked() {
                next = preview.position.checked_sub(1);
            }
            let forward = ui.add_enabled(preview.position.saturating_add(1) < preview.total, egui::Button::new(tr("Next historical image")));
            composer_widget(ui, "button:aiHistoricalNext", forward.rect);
            if !ui.is_sizing_pass() && forward.clicked() {
                next = preview.position.checked_add(1);
            }
        });
        if preview.available
            && let Ok(index) = usize::try_from(preview.index)
        {
            attachments.historical_image(ui, &format!("record-{}", preview.record), index, &preview.image);
        } else {
            ui.label(tr("Historical preview unavailable. Original reference is retained."));
        }
    });
    next
}
pub(super) fn read_native(reader: &mut HistoryReader, page: &HistorySlice, position: u64) -> Option<Preview> {
    let mut total = 0u64;
    let mut selected = None;
    for item in &page.items {
        let record = match *item {
            HistoryItem::Record { record, .. } | HistoryItem::Fragment { record, .. } => record,
            HistoryItem::Unavailable { .. } => continue,
        };
        if record >= reader.header().original_message_count() {
            continue;
        }
        let index = if selected.is_none() { position.saturating_sub(total) } else { u64::MAX };
        let (count, image) = match *item {
            HistoryItem::Record { index: message, .. } => {
                let images = page.records.get(message)?.get("images").and_then(Value::as_array);
                (
                    images.map_or(0, |images| images.len() as u64),
                    images.and_then(|images| usize::try_from(index).ok().and_then(|index| images.get(index))).cloned(),
                )
            }
            HistoryItem::Fragment { .. } => reader.original_image_reference(record, index).ok()?,
            HistoryItem::Unavailable { .. } => continue,
        };
        if selected.is_none()
            && position >= total
            && let Some(image) = image
        {
            let mime = image["mime_type"].as_str().unwrap_or_default().to_owned();
            let available = reader.preview(&image).is_ok();
            selected = Some(Preview { position, total: 0, record, index, mime, image, available });
        }
        total = total.checked_add(count)?;
    }
    selected.map(|mut preview| {
        preview.total = total;
        preview
    })
}
