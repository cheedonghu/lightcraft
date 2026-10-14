//! One captured preview reader; only visible image rows enqueue bounded work.
use super::*;
#[derive(Clone, PartialEq, Eq)]
struct Scope {
    generation: u64,
    photo: PhotoId,
    header: Option<lightcraft_engine::companion::storage::PhotoHeader>,
}
struct Request {
    scope: Scope,
    id: String,
    image: Value,
}
#[derive(Default)]
struct Retention {
    _history: Option<lightcraft_engine::companion::storage::HistoryRetention>,
    _legacy: Option<lightcraft_engine::companion::CompanionAttachmentRetention>,
}
enum Source {
    History(Box<lightcraft_engine::companion::storage::HistoryReader>),
    Legacy(lightcraft_engine::companion::CompanionAttachmentReader),
    Inline,
}
enum Preview {
    Ready(egui::TextureHandle, Arc<egui::ColorImage>, Retention),
    Missing,
}
struct Completed {
    request: Request,
    result: Result<(egui::ColorImage, Retention), String>,
}
pub(super) struct Attachments {
    scope: Option<Scope>,
    cache: BTreeMap<String, Preview>,
    order: VecDeque<String>,
    pending: VecDeque<Request>,
    visible: BTreeSet<String>,
    in_flight: Option<String>,
    alive: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    tx: mpsc::SyncSender<Completed>,
    rx: mpsc::Receiver<Completed>,
}
impl Default for Attachments {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(2);
        Self {
            scope: None,
            cache: BTreeMap::new(),
            order: VecDeque::new(),
            pending: VecDeque::new(),
            visible: BTreeSet::new(),
            in_flight: None,
            alive: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
        }
    }
}
impl Drop for Attachments {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Attachments {
    pub(super) fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.scope = None;
        self.cache.clear();
        self.order.clear();
        self.pending.clear();
        self.visible.clear();
        self.in_flight = None;
        while self.rx.try_recv().is_ok() {}
        // Actual producer still owns its reader until WorkerAlive drops.
    }
    pub(super) fn busy(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
    pub(super) fn sync(&mut self, generation: u64, photo: PhotoId, conversation: &Conversation) {
        self.visible.clear();
        let scope = Scope { generation, photo, header: conversation.prefix.clone() };
        if self.scope.as_ref() != Some(&scope) {
            self.cancel.store(true, Ordering::Relaxed);
            self.cache.clear();
            self.order.clear();
            self.pending.clear();
            self.in_flight = None;
            self.scope = Some(scope);
        }
    }
    pub(super) fn image(&mut self, ui: &mut egui::Ui, message_id: &str, index: usize, image: &Value) {
        self.show_image(ui, message_id, index, image, None);
    }
    pub(super) fn historical_image(&mut self, ui: &mut egui::Ui, message_id: &str, index: usize, image: &Value) {
        self.show_image(ui, message_id, index, image, Some("image:aiHistoricalPreview"));
    }
    fn show_image(&mut self, ui: &mut egui::Ui, message_id: &str, index: usize, image: &Value, alias: Option<&str>) {
        let width = ui.available_width().clamp(32.0, 240.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 168.0), egui::Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let label = format!("{message_id}:{index}");
        let bytes = serde_json::to_vec(image).unwrap_or_default();
        let id = format!("{:032x}", lightcraft_preview::hash_bytes(&bytes).0);
        self.visible.insert(id.clone());
        match self.cache.get(&id) {
            Some(Preview::Ready(texture, _pixels, _retention)) => {
                let size = texture.size_vec2();
                let fitted = size * (rect.width() / size.x).min(rect.height() / size.y);
                let image_rect = egui::Rect::from_center_size(rect.center(), fitted);
                ui.painter().image(texture.id(), image_rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                composer_widget(ui, &format!("image:aiAttachment:{label}"), image_rect);
                if let Some(alias) = alias {
                    composer_widget(ui, alias, image_rect);
                }
            }
            status => {
                ui.painter().rect_stroke(rect, 4.0, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
                let label = if matches!(status, Some(Preview::Missing)) { "Historical preview unavailable" } else { "Loading historical preview…" };
                let mut child = ui.new_child(
                    egui::UiBuilder::new().max_rect(rect.shrink(8.0)).layout(egui::Layout::centered_and_justified(egui::Direction::TopDown)),
                );
                child.label(tr(label));
                if status.is_none()
                    && self.pending.len() < 8
                    && self.in_flight.as_ref() != Some(&id)
                    && !self.pending.iter().any(|r| r.id == id)
                    && let Some(scope) = self.scope.clone()
                {
                    self.pending.push_back(Request { scope, id, image: image.clone() });
                    ui.ctx().request_repaint();
                }
            }
        }
    }
    pub(super) fn cpu_textures(&self, textures: &mut std::collections::HashMap<egui::TextureId, crate::softpaint::CpuTexture>) {
        for preview in self.cache.values() {
            if let Preview::Ready(texture, pixels, _) = preview {
                textures.insert(texture.id(), crate::softpaint::CpuTexture::linear(pixels.clone()));
            }
        }
    }
    fn insert(&mut self, id: String, preview: Preview) {
        self.order.retain(|key| key != &id);
        self.order.push_back(id.clone());
        self.cache.insert(id, preview);
        while self.order.len() > 8 {
            if let Some(key) = self.order.pop_front() {
                self.cache.remove(&key);
            }
        }
    }
    pub(super) fn poll(&mut self, app: &mut LightcraftApp, ctx: &egui::Context) {
        if self.scope.as_ref().is_some_and(|scope| scope.generation != app.session.library_generation()) {
            self.cancel.store(true, Ordering::Relaxed);
            self.scope = None;
            self.cache.clear();
            self.order.clear();
            self.pending.clear();
            self.visible.clear();
            self.in_flight = None;
        }
        self.pending.retain(|request| self.visible.contains(&request.id));
        // Completions remain owned until the actual captured reader has retired.
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        }
        while let Ok(message) = self.rx.try_recv() {
            if self.scope.as_ref() != Some(&message.request.scope) || message.request.scope.generation != app.session.library_generation() {
                continue;
            }
            self.in_flight = None;
            let preview = match message.result {
                Ok((image, retention)) => {
                    let image = Arc::new(image);
                    Preview::Ready(
                        ctx.load_texture(format!("ai-preview-{}", message.request.id), image.clone(), egui::TextureOptions::LINEAR),
                        image,
                        retention,
                    )
                }
                Err(_) => Preview::Missing,
            };
            self.insert(message.request.id, preview);
            ctx.request_repaint();
        }
        let Some(request) = self.pending.pop_front() else {
            return;
        };
        if self.scope.as_ref() != Some(&request.scope) || request.scope.generation != app.session.library_generation() {
            return;
        }
        let source = if request.image["data"].is_string() {
            Some(Source::Inline)
        } else if let Some(header) = request.scope.header.clone().or_else(|| {
            // A freshly persisted live tail has v2 references before the
            // sidebar owns a saved prefix. Capture its actual committed head;
            // the legacy reader only accepts v1 companion/preview paths.
            request.image["attachment"]
                .as_str()
                .filter(|name| name.starts_with("companion/v2/"))
                .and_then(|_| app.session.companion_photo_header(request.scope.photo).ok().flatten())
        }) {
            app.session.companion_owned_history_reader(&header).ok().flatten().map(|reader| Source::History(Box::new(reader)))
        } else {
            app.session.companion_attachment_reader(request.scope.photo).ok().flatten().map(Source::Legacy)
        };
        // Unsupported Stores remain local placeholders, never synchronous frame I/O.
        let Some(source) = source else {
            self.insert(request.id, Preview::Missing);
            return;
        };
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        self.alive.store(true, Ordering::Relaxed);
        let guard = WorkerAlive(self.alive.clone());
        let tx = self.tx.clone();
        let request_id = request.id.clone();
        self.in_flight = Some(request_id.clone());
        ctx.request_repaint_after(Duration::from_millis(50));
        let spawned = std::thread::Builder::new().name("companion-visible-preview".into()).spawn(move || {
            let _guard = guard;
            let mut source = source;
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let result = (|| {
                let bytes = match &mut source {
                    Source::History(reader) => {
                        let prefix = format!("companion/v2/photos/{}/previews/", request.scope.photo.0);
                        let legacy = format!("companion/preview-{}-", request.scope.photo.0);
                        if request.image["attachment"].as_str().is_none_or(|name| !name.starts_with(&prefix) && !name.starts_with(&legacy)) {
                            return Err("preview belongs to another photo".into());
                        }
                        reader.preview(&request.image).map_err(|e| e.to_string())?
                    }
                    Source::Legacy(reader) => reader.read(&request.image).map_err(|e| e.to_string())?,
                    Source::Inline => {
                        let encoded = request.image["data"].as_str().ok_or("preview missing")?;
                        if encoded.len() > 176 * 1024 {
                            return Err("encoded preview exceeds limits".into());
                        }
                        let bytes = Base64::decode_vec(encoded).map_err(|_| "invalid preview encoding")?;
                        if bytes.len() > 128 * 1024 {
                            return Err("preview exceeds limits".into());
                        }
                        bytes
                    }
                };
                lightcraft_engine::companion::validate_history_image(request.image["mime_type"].as_str().unwrap_or_default(), &bytes)?;
                let decoded =
                    lightcraft_codecs::decode(&bytes, lightcraft_codecs::DecodeOptions { max_size: Some((640, 640)), max_pixels: 640 * 640 })
                        .map_err(|e| e.to_string())?;
                let decoded = decoded.to_srgb8().into_oriented(lightcraft_geom::Orientation::from_exif(decoded.orientation));
                let image = egui::ColorImage::from_rgba_unmultiplied([decoded.width, decoded.height], &decoded.as_bytes());
                let retention = match source {
                    Source::History(reader) => Retention { _history: Some((*reader).into_retention()), ..Default::default() },
                    Source::Legacy(reader) => Retention { _legacy: Some(reader.into_retention()), ..Default::default() },
                    Source::Inline => Retention::default(),
                };
                Ok((image, retention))
            })();
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.try_send(Completed { request, result });
            }
        });
        if spawned.is_err() {
            self.in_flight = None;
            self.insert(request_id, Preview::Missing);
        }
    }
}
