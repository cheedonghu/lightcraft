//! Historical comparison through real archive and caller-facing navigation.
use super::*;
use crate::{Services, headless::Headless};
use lightcraft_catalog::MemStore;
use lightcraft_engine::{Session, companion::EditorState, library::LibraryStores};

#[test]
fn historical_round_comparison_keeps_full_composition_and_returns_without_replaying_effects() {
    let catalog = MemStore::new();
    let files = MemStore::new();
    let mut session = Session::new();
    session
        .open_library_in(LibraryStores { catalog: Box::new(catalog), files: Box::new(files), dir: "comparison".into(), on_disk: false }, true)
        .unwrap();
    let photo = session.active().unwrap();
    let round = LocalRoundId::new().unwrap();
    let original = session.develop_of(photo).unwrap();
    let mut actual = (*original).clone();
    actual.light.exposure = 0.35;
    let image = |width, height, color| {
        let bytes = lightcraft_engine::export::encode_image(
            &lightcraft_raster::Rgba8::filled(width, height, color),
            &lightcraft_engine::export::ExportOptions { format: lightcraft_engine::export::ExportFormat::Jpeg, quality: 75, ..Default::default() },
        )
        .unwrap();
        json!({"mime_type":"image/jpeg","data":Base64::encode_string(&bytes)})
    };
    let before_id = format!("{}-user", round.as_str());
    let after_id = format!("{}-initial", round.as_str());
    session
        .save_companion_conversation(
            photo,
            json!({"schema_version":2,"messages":[
                {"role":"user","text":"Saved request","local_round_id":round,"local_message_id":before_id,"display_kind":"request","complete_state":"complete","images":[image(640,320,[210,40,40,255])]},
                {"role":"user","text":"Saved complete execution","local_round_id":round,"local_message_id":after_id,"display_kind":"execution_feedback","complete_state":"complete","images":[image(320,640,[40,40,210,255])]}
            ],"rounds":[],"round_records":[{"local_round_id":round,"photo_id":photo.0,"before_preview_message_id":before_id,"original":original,"recent_complete_result":actual,"recent_complete_hash":actual.hash64(),"local_terminal":"completed","stages":[{"stage":"initial","plan_id":"saved-initial","reason":"Natural exposure","before_state":original,"applied_state":actual,"locally_published":true,"execution_outcome":"succeeded","preview_message_id":after_id}]}]}),
        )
        .unwrap();
    session.set_companion_editor(photo, EditorState { draft: "Keep the current unsent draft".into(), ..Default::default() }).unwrap();
    session.execute("develop.set", &json!({"control":"light.exposure","value":0.8})).unwrap();
    session.persist().unwrap();
    let current = session.develop_of(photo).unwrap();
    let mut h = Headless::new(LightcraftApp::new(session, Services::default()), [1400.0, 1000.0], 1.0);
    h.app.run("ai.open", json!({})).unwrap();
    h.app.run("ai.compare", json!({"local_round_id":round})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        let status = h.app.run("ai.status", json!({})).unwrap();
        if status["comparison"]["loading"] == false {
            assert_eq!(status["comparison"]["localRoundId"], round.as_str());
            assert_eq!(status["comparison"]["before"]["width"], 640);
            assert_eq!(status["comparison"]["before"]["height"], 320);
            assert_eq!(status["comparison"]["after"]["width"], 320);
            assert_eq!(status["comparison"]["after"]["height"], 640);
            break;
        }
        assert!(Instant::now() < deadline, "paired stored previews never became readable: {status}");
    }
    assert_eq!(
        h.app.run("ai.start", json!({"prompt":"cannot send from historical mode"})).unwrap_err(),
        tr("Return to the selected photo before starting a new AI task.")
    );
    assert_eq!(h.app.session.develop_of(photo).unwrap(), current);
    assert!(h.app.widgets.iter().any(|(id, _)| id == "image:aiCompareBefore"));
    assert!(h.app.widgets.iter().any(|(id, _)| id == "image:aiCompareAfter"));
    h.app.run("ai.returnCurrent", json!({})).unwrap();
    h.step();
    assert!(h.app.run("ai.status", json!({})).unwrap()["comparison"].is_null());
    assert_eq!(h.app.session.companion_editor(photo).unwrap().draft, "Keep the current unsent draft");
    assert_eq!(h.app.session.develop_of(photo).unwrap(), current);
}
