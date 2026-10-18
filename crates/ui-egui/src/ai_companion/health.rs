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
/// Settings flow vertically so translated status text cannot push the action out of place.
pub(super) fn show(app: &mut LightcraftApp, ui: &mut egui::Ui) {
    let short = match app.ai.health.state {
        "checking" => "Checking connection…",
        "reachable" => "Connected",
        "unreachable" => "Disconnected",
        _ => "Connection not checked",
    };
    let status = ui.label(tr(short));
    super::composer_widget(ui, "ai:healthStatus", status.rect);
    let details = ui.add(egui::Label::new(egui::RichText::new(tr(app.ai.health.label())).small().weak()).wrap());
    super::composer_widget(ui, "ai:healthDetails", details.rect);
    ui.add_space(4.0);
    let retry = ui.add(egui::Button::new(tr("Check Again")).min_size(egui::vec2(0.0, 30.0)));
    super::composer_widget(ui, "button:aiHealthRetry", retry.rect);
    if !ui.is_sizing_pass() && retry.clicked() {
        let _ = app.run("ai.health", json!({}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Services, headless::Headless};
    use lightcraft_engine::Session;

    #[test]
    fn settings_align_status_description_and_actions_in_both_languages_and_window_sizes() {
        for locale in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
            for state in ["reachable", "unreachable", "checking"] {
                for size in [[880.0, 600.0], [1400.0, 1000.0]] {
                    let mut app = LightcraftApp::new(Session::with_demo(), Services::default());
                    app.ui.language = locale;
                    app.ai.generation = app.session.library_generation();
                    app.ai.health.state = state;
                    app.ai.health.serial = 1;
                    app.ai.settings_open = true;
                    let mut h = Headless::new(app, size, 1.0);
                    h.app.run("ai.open", json!({})).unwrap();
                    for _ in 0..6 {
                        h.step();
                    }
                    assert_eq!(h.app.ai.health.status()["state"], state, "the layout fixture must retain the requested connection state");
                    let rect = |id: &str| h.app.widgets.iter().find(|(key, _)| key == id).unwrap_or_else(|| panic!("missing {id}")).1;
                    let window = rect("ai:settings");
                    let status = rect("ai:healthStatus");
                    let details = rect("ai:healthDetails");
                    let retry = rect("button:aiHealthRetry");
                    let clear = rect("button:aiClear");
                    for content in [status, details, retry, clear] {
                        assert!(window.contains_rect(content), "settings control clipped: {content:?} in {window:?}");
                    }
                    assert!((status.left() - details.left()).abs() <= 1.0);
                    assert!((status.left() - retry.left()).abs() <= 1.0);
                    assert!((retry.left() - clear.left()).abs() <= 1.0);
                    assert!(status.bottom() < details.top() && details.bottom() < retry.top());
                    assert!(clear.top() > retry.bottom() + 12.0, "conversation actions need their own section");
                    assert!(window.width() <= size[0] && window.bottom() <= size[1]);
                    if size[0] == 880.0 {
                        super::super::redesign_tests::screenshots(&mut h, &format!("settings-{state}-{}", locale.code()));
                    }
                }
            }
        }
    }
}
