use super::*;

#[test]
fn command_cannot_grant_consent() {
    let mut app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), crate::Services::default());
    let (connection, requests, _) = network::test_connection();
    app.ai.connection = Some(connection);
    app.ai.generation = app.session.library_generation();
    assert!(app.run("ai.start", json!({"consent":true})).is_err());
    assert!(!app.ai.consent);
    assert!(requests.try_recv().is_err());
}

#[test]
fn submission_is_global_and_explicit_photo_matches_selection() {
    let (mut app, state, photo, requests, _) = super::identity_tests::started();
    app.ai = state;
    let other = app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
    app.session.selection = lightcraft_engine::Selection::single(other);
    assert!(app.run("ai.start", json!({})).is_err());
    assert!(requests.try_recv().is_err());
    assert!(app.run("ai.start", json!({"photo":photo.0})).is_err());
}

#[test]
fn invalid_selection_and_unavailable_photos_never_enqueue() {
    for case in ["multi", "deleted", "missing", "explicit"] {
        let mut app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), crate::Services::default());
        let photo = app.session.active().unwrap();
        let other = app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
        let (connection, requests, _) = network::test_connection();
        app.ai = State { consent: true, generation: app.session.library_generation(), connection: Some(connection), ..Default::default() };
        match case {
            "multi" => app.session.selection.ids.push(other),
            "deleted" => {
                app.session.catalog.apply(lightcraft_catalog::Op::SetDeleted { id: photo, deleted: true }).unwrap();
            }
            "missing" => {
                let mut p = (**app.session.catalog.photo(photo).unwrap()).clone();
                p.source = lightcraft_catalog::Source::File { path: "/nonexistent-ux03-photo.png".into() };
                app.session.catalog.apply(lightcraft_catalog::Op::RemovePhoto { id: photo }).unwrap();
                app.session.catalog.apply(lightcraft_catalog::Op::AddPhoto { photo: Box::new(p) }).unwrap();
            }
            _ => {}
        }
        let params = if case == "explicit" { json!({"photo":other.0}) } else { json!({}) };
        assert!(app.run("ai.start", params).is_err(), "{case}");
        assert!(app.ai.jobs.is_empty());
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn clicked_start_is_locked_before_admission_and_failed_enqueue_releases() {
    let (mut app, mut state, photo, _, _) = super::identity_tests::started();
    assert_eq!(app.session.companion_task_photo(), Some(photo));
    assert!(app.session.execute("develop.set", &json!({"control":"light.exposure","value":1.0})).is_err());
    state.finish(photo, "rejected");
    assert!(app.session.companion_task_photo().is_none());
    let (connection, requests, _) = network::test_connection();
    drop(requests);
    state.connection = Some(connection);
    assert!(state.start(&mut app).is_err());
    assert!(app.session.companion_task_photo().is_none());
}

#[test]
fn real_checkbox_submission_and_other_photo_editing_headless() {
    for language in [crate::i18n::Locale::En, crate::i18n::Locale::ZhHans] {
        let mut app = LightcraftApp::new(
            lightcraft_engine::Session::with_demo(),
            crate::Services {
                write: Some(Box::new(lightcraft_engine::export::write_file)),
                png: Some(Box::new(|img| {
                    lightcraft_engine::export::encode_image(
                        img,
                        &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Png, ..Default::default() },
                    )
                    .unwrap()
                })),
                ..Default::default()
            },
        );
        let photo = app.session.active().unwrap();
        let other = app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
        app.ui.settings.gpu = false;
        app.ui.language = language;
        let (connection, requests, _events) = network::test_connection();
        app.ai.connection = Some(connection);
        app.ai.generation = app.session.library_generation();
        let mut h = crate::headless::Headless::new(app, [1600.0, 1000.0], 1.0);
        let timeout = Duration::from_secs(10);
        assert_eq!(h.request("ui.set", json!({"view":"detail","right":"edit","openSections":["light"]}), timeout)["ok"], true);
        h.app.run("ai.open", json!({})).unwrap();
        h.step();
        h.step();
        assert!(!h.app.ai.consent);
        let clicked = h.request("ui.clickWidget", json!({"id":"checkbox:aiConsent"}), timeout);
        assert_eq!(clicked["ok"], true, "{clicked}");
        assert!(h.app.ai.consent);
        h.app.ai.prompt = "Review this photo.".into();
        super::drafts::capture(&mut h.app);
        h.step();
        let clicked = h.request("ui.clickWidget", json!({"id":"button:aiStart"}), timeout);
        assert_eq!(clicked["ok"], true, "{clicked}");
        let request = requests.try_recv().unwrap();
        assert_eq!(request.path, "/v1/rounds");
        assert_eq!(h.app.session.companion_task_photo(), Some(photo));
        assert_eq!(h.app.ai.jobs.get(&photo).unwrap().state, "submitting");
        h.app.run("panel.edit", json!({})).unwrap();
        h.step();
        h.step();
        let before = h.app.session.develop_of(photo).unwrap();
        let click = h.request("ui.clickWidget", json!({"id":"slider:light.exposure","fx":0.9}), timeout);
        assert_eq!(click["ok"], true, "{click}");
        assert_eq!(h.app.session.develop_of(photo).unwrap(), before, "disabled edit control preserves click-time start");
        let locked_text = language.tr("Manual editing is paused while AI processes this photo; stop it or wait for completion.");
        assert!(h.painted_text().iter().any(|text| text.contains(locked_text)));
        if let Ok(dir) = std::env::var("UX03_SCREENSHOT_DIR") {
            let shot = h.request("ui.screenshot", json!({"path":format!("{dir}/submitting-{}.png",language.code()),"headless":true}), timeout);
            assert_eq!(shot["ok"], true, "{shot}");
            let inspect = h.request("ui.inspect", json!({}), timeout);
            assert_eq!(inspect["ok"], true, "{inspect}");
            assert!(inspect["result"]["perf"]["frameMs"].is_number());
            std::fs::write(format!("{dir}/inspect-submitting-{}.json", language.code()), serde_json::to_vec_pretty(&inspect).unwrap()).unwrap();
        }
        h.app.run("library.select", json!({"ids":[other.0],"active":other.0})).unwrap();
        h.step();
        h.step();
        assert!(h.app.ai.admission(&h.app, None).is_err());
        h.app.run("develop.set", json!({"control":"light.exposure","value":0.7})).unwrap();
        assert_eq!(h.app.session.develop_of(other).unwrap().light.exposure, 0.7);
        assert_eq!(h.app.session.develop_of(photo).unwrap(), before);
        h.step();
        h.step();
        assert!(!h.painted_text().iter().any(|text| text.contains(locked_text)));
        if let Ok(dir) = std::env::var("UX03_SCREENSHOT_DIR") {
            let shot = h.request("ui.screenshot", json!({"path":format!("{dir}/other-photo-{}.png",language.code()),"headless":true}), timeout);
            assert_eq!(shot["ok"], true, "{shot}");
        }
    }
}

#[test]
fn queued_processing_and_confirmed_rejection_keep_one_instance_owner() {
    for phase in ["queued", "awaiting_initial_response"] {
        let (mut app, mut state, photo, requests, events) = super::identity_tests::started();
        let identity = state.jobs.get(&photo).unwrap().identity.clone();
        let other = app.session.catalog.photos().find(|p| p.id != photo).unwrap().id;
        state.server(&mut app,&egui::Context::default(),"round.state",&json!({"round_id":"round","payload":{"submission_seq":identity.submission_seq,"photo_ref":{"photo_id":photo.0,"library_generation":identity.scope.library_generation},"state":phase}})).unwrap();
        assert_eq!(app.session.companion_task_photo(), Some(photo));
        app.session.selection = lightcraft_engine::Selection::single(other);
        assert!(state.start(&mut app).is_err());
        assert!(requests.try_recv().is_err());
        state.stop(photo).unwrap();
        assert!(app.session.companion_task_photo().is_none());
        assert!(requests.try_recv().unwrap().path.ends_with("/stop"));
        drop(events);
    }
    let (mut app, mut state, photo, _, events) = super::identity_tests::started();
    let identity = state.jobs.get(&photo).unwrap().identity.clone();
    let mut failure: network::RequestFailure = "queue full".into();
    failure.rejected = true;
    events
        .try_send(network::Event::Response {
            submission: identity.submission_seq,
            round: String::new(),
            path: "/v1/rounds".into(),
            identity,
            result: Err(failure),
        })
        .unwrap();
    state.poll_inner(&mut app, &egui::Context::default());
    assert!(state.jobs.is_empty());
    assert!(app.session.companion_task_photo().is_none());
    assert!(state.connection.is_some(), "confirmed rejection keeps the healthy connection");
}
