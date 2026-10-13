//! HTTP reachability, independent from model progress and run identity.
use super::{LightcraftApp, network, tr};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub(super) struct Health {
    serial: u64,
    state: &'static str,
    error: Option<String>,
    pending: Option<(u64, String)>,
    workers: Vec<(Arc<AtomicBool>, Arc<AtomicBool>)>,
    tx: mpsc::SyncSender<(u64, Result<(), String>)>,
    rx: mpsc::Receiver<(u64, Result<(), String>)>,
}
impl Default for Health {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(4);
        Self { serial: 0, state: "unchecked", error: None, pending: None, workers: Vec::new(), tx, rx }
    }
}
impl Drop for Health {
    fn drop(&mut self) {
        for (cancel, _) in &self.workers {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}
struct Alive(Arc<AtomicBool>);
impl Drop for Alive {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl Health {
    pub fn first_open(&mut self, address: String) {
        if self.state == "unchecked" {
            self.request(address);
        }
    }
    pub fn request(&mut self, address: String) {
        let Some(serial) = self.serial.checked_add(1) else {
            self.state = "unreachable";
            self.error = Some("Restart LightCraft to check the AI service again.".into());
            return;
        };
        self.serial = serial;
        self.state = "checking";
        self.error = None;
        for (cancel, _) in &self.workers {
            cancel.store(true, Ordering::Relaxed);
        }
        self.pending = Some((serial, address));
        self.poll();
    }
    pub fn checking(&self) -> bool {
        self.state == "checking"
    }
    pub fn poll(&mut self) {
        while let Ok((serial, result)) = self.rx.try_recv() {
            if serial == self.serial {
                self.state = if result.is_ok() { "reachable" } else { "unreachable" };
                self.error = result.err();
            }
        }
        self.workers.retain(|(_, alive)| alive.load(Ordering::Acquire));
        if self.workers.len() >= 2 {
            return;
        }
        let Some((serial, address)) = self.pending.take() else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let guard = Alive(alive.clone());
        let cancelled = cancel.clone();
        let tx = self.tx.clone();
        match std::thread::Builder::new().name("photo-companion-health".into()).spawn(move || {
            let _guard = guard;
            let result = network::health(&address, &cancelled);
            let _ = tx.try_send((serial, result));
        }) {
            Ok(_) => self.workers.push((cancel, alive)),
            Err(_) => {
                self.state = "unreachable";
                self.error = Some("The local AI service check could not start. Try again.".into());
            }
        }
    }
    pub fn status(&self) -> Value {
        json!({"state": self.state, "probe": self.serial, "diagnostic": self.error})
    }
    fn label(&self) -> &'static str {
        match self.state {
            "checking" => "Checking the local AI service…",
            "reachable" => "AI service reachable. Model availability is checked when you submit.",
            "unreachable" => "Cannot reach the local AI service. Start the backend, then check again.",
            _ => "Check the local AI service before submitting.",
        }
    }
}
/// The status row and optional details viewport have stable geometry while a probe finishes.
/// Compact Tools keep the status/retry row in the composer and details below history.
pub(super) fn show(app: &mut LightcraftApp, ui: &mut egui::Ui, compact: bool) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let retry = ui.button(tr("Check Again"));
            super::composer_widget(ui, "button:aiHealthRetry", retry.rect);
            if !ui.is_sizing_pass() && retry.clicked() {
                let _ = app.run("ai.health", json!({}));
            }
            let short = match app.ai.health.state {
                "checking" => "Checking service…",
                "reachable" => "Service reachable (HTTP only)",
                "unreachable" => "Service unreachable",
                _ => "Service not checked",
            };
            ui.add(egui::Label::new(tr(short)).truncate()).on_hover_text(tr(app.ai.health.label()));
        });
    });
    if !compact {
        details(app, ui);
    }
}
pub(super) fn details(app: &LightcraftApp, ui: &mut egui::Ui) {
    let area =
        egui::ScrollArea::vertical().id_salt("ai-service-details").auto_shrink([false, false]).min_scrolled_height(48.0).max_height(48.0).show(
            ui,
            |ui| {
                ui.label(tr(app.ai.health.label()));
                if app.ai.health.state == "unreachable" {
                    let guide = if cfg!(target_os = "windows") {
                        "In photo-companion, run scripts/start-backend.ps1."
                    } else {
                        "In photo-companion, run: go run ./cmd/photo-companion --log-file logs/backend.log"
                    };
                    ui.label(tr(guide));
                }
            },
        );
    super::composer_widget(ui, "ai:healthDetails", area.inner_rect);
}
