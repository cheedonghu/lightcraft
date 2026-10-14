use lightcraft_catalog::{MemStore,PhotoId};
use lightcraft_engine::{Session,library::LibraryStores,companion::{EditorState,storage::HistoryCursor}};
use lightcraft_ui_egui::{LightcraftApp,Services,headless::Headless};
use serde_json::json;
use std::time::{Instant,Duration};
fn fixture(name:&str)->(Session,MemStore,MemStore,PhotoId){
 let core=MemStore::new();let files=MemStore::new();let mut s=Session::new();
 s.open_library_in(LibraryStores{catalog:Box::new(core.clone()),files:Box::new(files.clone()),dir:name.into(),on_disk:false},true).unwrap();
 let p=s.active().unwrap();
 s.save_companion_tail(p,None,&[json!({"role":"assistant","local_message_id":format!("{name}-giant"),"text":format!("{}END_{name}","readable sentence α🙂.\n".repeat(30000))})],&[],&[],&json!({"schema_version":2})).unwrap();
 let head=s.companion_photo_header(p).unwrap().unwrap();
 s.set_companion_editor(p,EditorState{draft:format!("draft-{name}"),follow_tail:false,reading_cursor:Some(HistoryCursor{record:0,offset:0}),reading_epoch:Some(head.conversation_epoch),reading_count:Some(head.message_count),..Default::default()}).unwrap();
 s.persist().unwrap();(s,core,files,p)
}
fn settle(h:&mut Headless){let start=Instant::now();loop{h.step();let status=h.app.run("ai.status",json!({})).unwrap();if status["historyLoading"]==false{break;}assert!(start.elapsed()<Duration::from_secs(15));std::thread::yield_now();}for _ in 0..3{h.step();}}
fn main(){for pending in [false,true]{
 let (a,_,_,pa)=fixture("A");let (mut b,core,files,pb)=fixture("B");assert_eq!(pa,pb);b.close_library().unwrap();drop(b);
 let mut h=Headless::new(LightcraftApp::new(a,Services::default()),[1400.0,1000.0],1.0);
 h.app.run("ai.open",json!({})).unwrap();settle(&mut h);
 if pending {h.app.run("ai.bottom",json!({})).unwrap();}
 h.app.session.open_library_in(LibraryStores{catalog:Box::new(core),files:Box::new(files),dir:"B".into(),on_disk:false},true).unwrap();
 // First real frame owns generation transition: no AI command before polling.
 h.step();settle(&mut h);
 let ed=h.app.session.companion_editor(pb).unwrap();println!("pending_A={pending} B_editor={ed:?}");
 assert_eq!(ed.draft,"draft-B");
 assert_eq!(ed.reading_cursor,Some(HistoryCursor{record:0,offset:0}),"A's pending bottom continuation leaked to B");
 }}
