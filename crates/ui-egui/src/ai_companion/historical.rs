//! A single original historical image, without inferred before/after pairing.
//! Source bytes are validated before decoding; one bounded image owns its texture.
use super::*;
use lightcraft_engine::companion::storage::{HistoryItem, HistorySlice};
pub(super) struct Preview {
    pub position: u64,
    pub total: u64,
    pub record: u64,
    pub index: u64,
    pub mime: String,
    rgba: Option<lightcraft_raster::Rgba8>,
    texture: Option<egui::TextureHandle>,
}
impl Preview {
    pub fn status(&self) -> Value {
        json!({"position":self.position,"total":self.total,"record":self.record,"index":self.index,"mime_type":self.mime,"available":self.rgba.is_some()})
    }
    pub fn bytes(&self) -> usize {
        self.rgba.as_ref().map_or(0, |image| image.data.len().saturating_mul(4))
    }
}
pub(super) fn read(page: &HistorySlice, position: u64, mut bytes: impl FnMut(&Value) -> Result<Vec<u8>, String>) -> Option<Preview> {
    let mut total = 0u64;
    let mut selected = None;
    for item in &page.items {
        if let HistoryItem::Record { record, index } = *item
            && let Some(message) = page.records.get(index)
            && let Some(images) = message.get("images").and_then(Value::as_array)
        {
            for (image_index, image) in images.iter().enumerate() {
                if total == position {
                    selected = Some((record, image_index as u64, image));
                }
                total = total.saturating_add(1);
            }
        }
    }
    let (record, index, image) = selected?;
    let mime = image.get("mime_type").and_then(Value::as_str).unwrap_or_default().to_owned();
    let rgba = bytes(image).ok().and_then(|bytes| {
        let options = lightcraft_codecs::ThumbnailOptions { max_edge: 160, min_embedded_edge: 160, max_pixels: 640 * 640 };
        lightcraft_codecs::decode_thumbnail_with(&bytes, &options)
            .ok()
            .map(|preview| preview.image.into_oriented(lightcraft_geom::Orientation::from_exif(preview.orientation)))
    });
    Some(Preview { position, total, record, index, mime, rgba, texture: None })
}
pub(super) fn show(ui: &mut egui::Ui, preview: &mut Preview) -> Option<u64> {
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
        if let Some(image) = &preview.rgba {
            if preview.texture.is_none() {
                let pixels: Vec<_> =
                    image.data.iter().copied().map(|pixel| egui::Color32::from_rgba_unmultiplied(pixel[0], pixel[1], pixel[2], pixel[3])).collect();
                let color = egui::ColorImage::new([image.width, image.height], pixels);
                preview.texture =
                    Some(ui.ctx().load_texture(format!("historical:{}:{}", preview.record, preview.index), color, egui::TextureOptions::LINEAR));
            }
            if let Some(texture) = &preview.texture {
                let response = ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(image.width as f32, image.height as f32)));
                composer_widget(ui, "image:aiHistoricalPreview", response.rect);
            }
        } else {
            ui.label(tr("Historical preview unavailable. Original reference is retained."));
        }
    });
    next
}

pub(super) fn read_native(reader: &mut lightcraft_engine::companion::storage::HistoryReader, page: &HistorySlice, position: u64) -> Option<Preview> {
    let mut total = 0u64;
    let mut selected = None;
    for item in &page.items {
        let record = match *item {
            HistoryItem::Record { record, .. } | HistoryItem::Fragment { record, .. } => record,
        };
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
        };
        if selected.is_none()
            && position >= total
            && let Some(image) = image
        {
            let mime = image.get("mime_type").and_then(Value::as_str).unwrap_or_default().to_owned();
            let rgba = reader.preview(&image).ok().and_then(|bytes| {
                let options = lightcraft_codecs::ThumbnailOptions { max_edge: 160, min_embedded_edge: 160, max_pixels: 640 * 640 };
                lightcraft_codecs::decode_thumbnail_with(&bytes, &options)
                    .ok()
                    .map(|preview| preview.image.into_oriented(lightcraft_geom::Orientation::from_exif(preview.orientation)))
            });
            selected = Some(Preview { position, total: 0, record, index, mime, rgba, texture: None });
        }
        total = total.checked_add(count)?;
    }
    selected.map(|mut selected| {
        selected.total = total;
        selected
    })
}
