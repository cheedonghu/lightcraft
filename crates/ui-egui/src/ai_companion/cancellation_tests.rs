use super::*;

#[test]
fn revoke_then_reconsent_never_revives_the_old_submission_or_content() {
    let (mut app, mut state, photo, requests, events) = super::identity_tests::started();
    let identity = state.jobs.get(&photo).unwrap().identity.clone();
    let gate = state.jobs.get(&photo).unwrap().cancel.clone();
    state.conversations.get_mut(&photo).unwrap().pending.insert("partial".into(), "unconfirmed".into());
    let start = std::time::Instant::now();
    state.set_consent(false).unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(!state.consent);
    assert!(gate.load(Ordering::Relaxed));
    assert!(state.conversations.get(&photo).unwrap().pending.is_empty());
    state.set_consent(true).unwrap();
    assert!(gate.load(Ordering::Relaxed));
    assert!(state.start(&mut app).is_err(), "unknown submission still owns its identity");
    assert!(state.send("/v1/rounds/unknown/input".into(), json!({"preview":"must not upload"}), identity.submission_seq, String::new()).is_err());
    let mut cancelled: network::RequestFailure = "cancelled before any delivery".into();
    cancelled.cancelled_before_send = true;
    events
        .try_send(network::Event::Response {
            submission: identity.submission_seq,
            round: String::new(),
            path: "/v1/rounds".into(),
            identity: identity.clone(),
            result: Err(cancelled),
        })
        .unwrap();
    state.poll_inner(&mut app, &egui::Context::default());
    assert!(!state.jobs.contains_key(&photo));
    assert!(app.session.companion_task_photo().is_none());
    let notice = state.conversations.get(&photo).unwrap().notice.clone();
    events
        .try_send(network::Event::Server { kind: "round.ended".into(), data: json!({"round_id":"old","payload":{"terminal_state":"completed"}}) })
        .unwrap();
    state.poll_inner(&mut app, &egui::Context::default());
    assert_eq!(state.conversations.get(&photo).unwrap().notice, notice, "closed only once");
    state.start(&mut app).unwrap();
    let next = requests.try_recv().unwrap();
    assert_ne!(next.identity.local_round_id, identity.local_round_id);
    assert!(!next.cancel.load(Ordering::Relaxed));
    assert!(gate.load(Ordering::Relaxed));
}

#[test]
fn uncertain_creation_blocks_new_tasks_until_the_old_connection_workers_close() {
    let (mut app, mut state, photo, requests, events) = super::identity_tests::started();
    let identity = state.jobs.get(&photo).unwrap().identity.clone();
    events
        .try_send(network::Event::Response {
            submission: identity.submission_seq,
            round: String::new(),
            path: "/v1/rounds".into(),
            identity: identity.clone(),
            result: Err("response lost after delivery".into()),
        })
        .unwrap();
    state.poll_inner(&mut app, &egui::Context::default());
    assert!(state.jobs.get(&photo).unwrap().stopped);
    assert!(state.connection.as_ref().unwrap().is_closing());
    assert_eq!(app.session.companion_task_photo(), Some(photo));
    assert!(state.start(&mut app).is_err());
    assert!(requests.try_recv().is_err());
    // The real transport sets alive only after joining its request worker.
    state.connection.as_ref().unwrap().alive.store(false, Ordering::Relaxed);
    state.poll_inner(&mut app, &egui::Context::default());
    assert!(state.jobs.is_empty());
    assert!(app.session.companion_task_photo().is_none());
}

#[test]
fn actual_checkbox_can_revoke_during_a_queued_task_in_both_locales() {
    for locale in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let (mut app, state, photo, requests, events) = super::identity_tests::started();
        app.services.write = Some(Box::new(lightcraft_engine::export::write_file));
        app.services.png = Some(Box::new(|img| {
            lightcraft_engine::export::encode_image(
                img,
                &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
            )
            .unwrap()
        }));
        app.ai = state;
        app.ai.open = true;
        app.ui.settings.gpu = false;
        app.ui.language = locale;
        let identity = app.ai.jobs.get(&photo).unwrap().identity.clone();
        let gate = app.ai.jobs.get(&photo).unwrap().cancel.clone();
        events.try_send(network::Event::Server { kind: "round.state".into(), data: json!({"round_id":"round","payload":{"submission_seq":identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":identity.scope.library_generation},"state":"queued"}}) }).unwrap();
        let mut h = crate::headless::Headless::new(app, [1600.0, 1000.0], 1.0);
        h.step();
        h.step();
        let timeout = Duration::from_secs(10);
        let before = h.app.session.develop_of(photo).unwrap();
        if let Ok(dir) = std::env::var("UX04_SCREENSHOT_DIR") {
            let shot = h.request("ui.screenshot", json!({"path":format!("{dir}/queued-{}.png",locale.code()),"headless":true}), timeout);
            assert_eq!(shot["ok"], true, "{shot}");
        }
        let clicked = h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout);
        assert_eq!(clicked["ok"], true, "{clicked}");
        assert!(!h.app.ai.consent);
        assert!(gate.load(Ordering::Relaxed));
        let stop = requests.try_recv().unwrap();
        assert!(stop.path.ends_with("/stop"));
        assert_eq!(h.app.session.develop_of(photo).unwrap(), before);
        assert!(!h.app.ai.jobs.contains_key(&photo));
        if let Ok(dir) = std::env::var("UX04_SCREENSHOT_DIR") {
            h.step();
            h.step();
            let shot = h.request("ui.screenshot", json!({"path":format!("{dir}/revoked-{}.png",locale.code()),"headless":true}), timeout);
            assert_eq!(shot["ok"], true, "{shot}");
            let inspect = h.request("ui.inspect", json!({}), timeout);
            assert_eq!(inspect["ok"], true);
            std::fs::write(format!("{dir}/revoked-{}.json", locale.code()), serde_json::to_vec_pretty(&inspect).unwrap()).unwrap();
        }
    }
}

#[test]
fn candidate_publish_before_or_after_revocation_obeys_local_order_and_keeps_real_success() {
    for publish_first in [false, true] {
        let (mut app, mut state, photo, requests, _) = super::identity_tests::started();
        let identity = state.jobs.get(&photo).unwrap().identity.clone();
        let original = app.session.develop_of(photo).unwrap();
        let ctx = egui::Context::default();
        let event = |phase: &str| json!({"round_id":"round","payload":{"submission_seq":identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":identity.scope.library_generation},"state":phase}});
        state.server(&mut app, &ctx, "round.state", &event("awaiting_input")).unwrap();
        let initial = state.rx.recv_timeout(Duration::from_secs(30)).unwrap();
        state.prepared(&mut app, &ctx, initial).unwrap();
        requests.try_recv().unwrap();
        state.server(&mut app, &ctx, "round.state", &event("awaiting_initial_execution")).unwrap();
        let revision = app.session.photo_revision(photo);
        state.server(&mut app, &ctx, "edit.plan", &json!({"round_id":"round","payload":{"plan_id":"initial","capability_version":"lightcraft-v2","expected_revision":revision,"operations":[{"type":"set_controls","values":{"light.exposure":0.4}}]}})).unwrap();
        let candidate = state.rx.recv_timeout(Duration::from_secs(30)).unwrap();
        let gate = state.jobs.get(&photo).unwrap().cancel.clone();
        if publish_first {
            state.prepared(&mut app, &ctx, candidate).unwrap();
            assert_eq!(app.session.develop_of(photo).unwrap().light.exposure, 0.4);
            let feedback = requests.try_recv().unwrap();
            assert!(feedback.path.ends_with("/execution"));
            state.set_consent(false).unwrap();
            assert!(feedback.cancel.load(Ordering::Relaxed), "unsent actual feedback also stops uploading without reverting success");
        } else {
            state.set_consent(false).unwrap();
            state.prepared(&mut app, &ctx, candidate).unwrap();
            assert_eq!(app.session.develop_of(photo).unwrap(), original);
        }
        assert!(gate.load(Ordering::Relaxed));
        assert!(!state.jobs.contains_key(&photo));
        assert!(requests.try_recv().unwrap().path.ends_with("/stop"));
        let kept = app.session.develop_of(photo).unwrap();
        let notice = state.conversations.get(&photo).unwrap().notice.clone();
        state.server(&mut app, &ctx, "edit.plan", &json!({"round_id":"round","payload":{"plan_id":"late"}})).unwrap();
        state.server(&mut app, &ctx, "round.ended", &json!({"round_id":"round","payload":{"terminal_state":"completed"}})).unwrap();
        assert_eq!(app.session.develop_of(photo).unwrap(), kept);
        assert_eq!(state.conversations.get(&photo).unwrap().notice, notice);
        assert!(requests.try_recv().is_err());
        state.save(&mut app, photo).unwrap();
    }
}
