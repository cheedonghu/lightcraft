//! Actual filesystem ownership can outlive membership in an old history Op.
use super::*;
use crate::Selection;
use crate::companion::LocalRoundId;

fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("lc-ux03-history-{}", LocalRoundId::new().unwrap().as_str()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn set_source(s: &mut Session, id: PhotoId, path: &Path) {
    s.apply_system(Op::SetFile {
        id,
        file_name: path.file_name().unwrap().to_string_lossy().into(),
        source: Source::File { path: path.to_string_lossy().into() },
    })
    .unwrap();
}

fn files(path: &Path) -> Vec<(String, Vec<u8>)> {
    fn collect(root: &Path, dir: &Path, result: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            let name = path.strip_prefix(root).unwrap().to_string_lossy().into();
            if kind.is_symlink() {
                result.push((name, std::fs::read_link(path).unwrap().to_string_lossy().as_bytes().to_vec()));
            } else if kind.is_dir() {
                result.push((name, Vec::new()));
                collect(root, &path, result);
            } else {
                result.push((name, std::fs::read(path).unwrap()));
            }
        }
    }
    let mut result = Vec::new();
    collect(path, path, &mut result);
    result.sort();
    result
}

fn state(s: &Session) -> String {
    let photos: Vec<_> =
        s.catalog.photos().map(|p| (p.id, lightcraft_preview::hash_bytes(&serde_json::to_vec(p).unwrap()), s.photo_revision(p.id))).collect();
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        photos, s.catalog.revision, s.undo, s.redo, s.pending_sidecar_repairs, s.pending_log, s.interaction, s.selection
    )
}

fn assert_rejected_without_effects(s: &mut Session, root: &Path, redo: bool) {
    let before_state = state(s);
    let before_files = files(root);
    let result = if redo { s.redo_step() } else { s.undo_step() };
    assert_eq!(state(s), before_state, "rejection must preserve catalog/revisions/parameters/stacks/repairs/log/interaction/selection: {result:?}");
    assert_eq!(files(root), before_files, "rejection must precede all physical repairs/moves: {result:?}");
    assert!(result.unwrap_err().to_string().contains("Manual editing is paused"));
}

fn rename_fixture(redo: bool) -> (Session, PathBuf, PhotoId, PhotoId, PathBuf) {
    let root = root();
    let old = root.join("old.png");
    let shared = root.join("shared.png");
    std::fs::write(&old, b"original procedural source").unwrap();
    let mut s = Session::with_demo();
    let a = s.active().unwrap();
    let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
    set_source(&mut s, a, &shared);
    set_source(&mut s, b, &old);
    let plans = s.plan_rename(&[b], "shared", 1);
    s.apply_rename(&plans).unwrap();
    if redo {
        s.undo_step().unwrap();
        set_source(&mut s, a, &old);
    }
    let entry = if redo { s.redo.last() } else { s.undo.last() }.unwrap();
    let mut includes_task = false;
    entry.op.photo_ids(&mut |id| includes_task |= id == a);
    assert!(!includes_task);
    let task_source = if redo { old } else { shared };
    (s, root, a, b, task_source)
}

#[test]
fn history_file_aliases_reject_before_pop_repairs_and_physical_moves() {
    for redo in [false, true] {
        for alias in ["literal", "canonical", "case", "symlink"] {
            let (mut s, root, a, b, source) = rename_fixture(redo);
            let task_source = match alias {
                "canonical" => {
                    std::fs::create_dir(root.join("nested")).unwrap();
                    root.join("nested").join("..").join(source.file_name().unwrap())
                }
                "case" => {
                    let path = root.join(source.file_name().unwrap().to_string_lossy().to_uppercase());
                    if !path.exists() {
                        std::fs::remove_dir_all(root).unwrap();
                        continue;
                    }
                    path
                }
                "symlink" => {
                    #[cfg(unix)]
                    {
                        std::os::unix::fs::symlink(&root, root.join("alias")).unwrap();
                        root.join("alias").join(source.file_name().unwrap())
                    }
                    #[cfg(not(unix))]
                    {
                        source.clone()
                    }
                }
                _ => source.clone(),
            };
            set_source(&mut s, a, &task_source);
            let _lease = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
            s.selection = Selection::single(b);
            s.begin_interaction("preserve independent interaction").unwrap();
            assert_rejected_without_effects(&mut s, &root, redo);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn history_folder_ancestry_rejects_a_task_added_after_the_original_op() {
    for redo in [false, true] {
        for alias in ["direct", "directory_alias", "file_alias"] {
            let root = root();
            let old = root.join("old");
            let moved = root.join("moved");
            std::fs::create_dir(&old).unwrap();
            std::fs::write(old.join("b.png"), b"B source").unwrap();
            let mut s = Session::with_demo();
            let a = s.active().unwrap();
            let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
            set_source(&mut s, b, &old.join("b.png"));
            crate::cmd::browse::move_folder(&mut s, &old.to_string_lossy(), &moved.to_string_lossy()).unwrap();
            if redo {
                s.undo_step().unwrap();
            }
            let current = if redo { &old } else { &moved };
            if alias == "file_alias" {
                #[cfg(unix)]
                {
                    let outside = root.join("outside.png");
                    std::fs::write(&outside, b"outside source").unwrap();
                    std::os::unix::fs::symlink(&outside, current.join("a.png")).unwrap();
                }
                #[cfg(not(unix))]
                {
                    std::fs::write(current.join("a.png"), b"new independent task source").unwrap();
                }
            } else {
                std::fs::write(current.join("a.png"), b"new independent task source").unwrap();
            }
            let task = if alias == "directory_alias" {
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(current, root.join("alias")).unwrap();
                    root.join("alias/a.png")
                }
                #[cfg(not(unix))]
                {
                    current.join("a.png")
                }
            } else {
                current.join("a.png")
            };
            set_source(&mut s, a, &task);
            let entry = if redo { s.redo.last() } else { s.undo.last() }.unwrap();
            let mut includes_task = false;
            entry.op.photo_ids(&mut |id| includes_task |= id == a);
            assert!(!includes_task);
            let _lease = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
            s.selection = Selection::single(b);
            s.begin_interaction("unrelated B gesture").unwrap();
            assert_rejected_without_effects(&mut s, &root, redo);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn chained_folder_aliases_reject_forward_and_history_without_effects() {
    for mode in ["forward", "undo", "redo"] {
        for relative in [false, true] {
            let root = root();
            let old = root.join("old");
            let moved = root.join("moved");
            let outside = root.join("outside");
            std::fs::create_dir(&old).unwrap();
            std::fs::create_dir(&outside).unwrap();
            std::fs::write(old.join("b.png"), b"B source").unwrap();
            std::fs::write(outside.join("a.png"), b"A source").unwrap();
            let mut s = Session::with_demo();
            let a = s.active().unwrap();
            let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
            set_source(&mut s, b, &old.join("b.png"));
            if mode != "forward" {
                crate::cmd::browse::move_folder(&mut s, &old.to_string_lossy(), &moved.to_string_lossy()).unwrap();
            }
            if mode == "redo" {
                s.undo_step().unwrap();
            }
            let current = if mode == "undo" { &moved } else { &old };
            let link_target = if relative { PathBuf::from("../outside") } else { outside.clone() };
            std::os::unix::fs::symlink(link_target, current.join("link")).unwrap();
            let task = root.join("task.png");
            let task_target = if relative { current.strip_prefix(&root).unwrap().join("link/a.png") } else { current.join("link/a.png") };
            std::os::unix::fs::symlink(task_target, &task).unwrap();
            set_source(&mut s, a, &task);
            let _lease = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
            s.selection = Selection::single(b);
            s.begin_interaction("independent B gesture").unwrap();
            assert_eq!(std::fs::read(&task).unwrap(), b"A source");
            if mode == "forward" {
                let before_state = state(&s);
                let before_files = files(&root);
                let result = crate::cmd::browse::move_folder(&mut s, &old.to_string_lossy(), &moved.to_string_lossy());
                assert_eq!(state(&s), before_state, "forward rejection must preserve all session state: {result:?}");
                assert_eq!(files(&root), before_files, "forward rejection must preserve physical traversal entries: {result:?}");
                assert!(result.unwrap_err().to_string().contains("Manual editing is paused"));
            } else {
                assert_rejected_without_effects(&mut s, &root, mode == "redo");
            }
            assert_eq!(std::fs::read(&task).unwrap(), b"A source");
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

fn repair(from: PathBuf, to: PathBuf) -> SidecarMove {
    SidecarMove {
        companions: vec![RetainedPacket { version: Some(RealFs.packet_version(&from).unwrap()), path: from.clone() }],
        from,
        to,
        copied: false,
    }
}

#[test]
fn file_alias_history_preflight_preserves_detached_global_and_other_history_repairs() {
    for redo in [false, true] {
        for queue in ["detached", "global", "other_history"] {
            let (mut s, root, a, _, _) = rename_fixture(redo);
            let from = root.join("repair-from.xmp");
            let to = root.join("repair-to.xmp");
            std::fs::write(&from, b"retained packet").unwrap();
            let repair = repair(from, to);
            match queue {
                "global" => s.pending_sidecar_repairs.push(repair),
                "other_history" => {
                    let entry = crate::UndoEntry {
                        label: "other repair".into(),
                        op: Op::Batch { ops: Vec::new() },
                        folder: None,
                        retry_inverse: Vec::new(),
                        retry_sidecars: vec![repair],
                    };
                    if redo {
                        s.undo.push(entry);
                    } else {
                        s.redo.push(entry);
                    }
                }
                _ => {
                    if redo { s.redo.last_mut() } else { s.undo.last_mut() }.unwrap().retry_sidecars.push(repair);
                }
            }
            let _lease = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
            assert_rejected_without_effects(&mut s, &root, redo);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn repair_original_and_sidecar_endpoints_are_in_the_same_physical_scope() {
    for redo in [false, true] {
        for endpoint in ["original_source", "original_destination", "stem_source", "stem_destination", "full_source", "full_destination"] {
            let (mut s, root, a, _, _) = rename_fixture(redo);
            let task = root.join("task.png");
            std::fs::write(&task, b"task original").unwrap();
            set_source(&mut s, a, &task);
            let packet = match endpoint {
                "stem_source" | "stem_destination" => task.with_extension("xmp"),
                "full_source" | "full_destination" => PathBuf::from(format!("{}.xmp", task.display())),
                _ => task.clone(),
            };
            let (from, to) = if endpoint.ends_with("source") {
                std::fs::write(&packet, b"task bytes").unwrap();
                (packet, root.join("repair-endpoint.xmp"))
            } else {
                let from = root.join("repair-start.xmp");
                std::fs::write(&from, b"retained bytes").unwrap();
                (from, packet)
            };
            let repair = repair(from, to);
            if redo { s.redo.last_mut() } else { s.undo.last_mut() }.unwrap().retry_sidecars.push(repair);
            let _lease = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
            assert_rejected_without_effects(&mut s, &root, redo);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn independent_file_and_folder_history_remains_available() {
    for redo in [false, true] {
        let (mut s, root, a, _, _) = rename_fixture(redo);
        let task = root.join("independent.png");
        std::fs::write(&task, b"unrelated task").unwrap();
        set_source(&mut s, a, &task);
        let _lease = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
        if redo { s.redo_step() } else { s.undo_step() }.unwrap();
        assert_eq!(std::fs::read(&task).unwrap(), b"unrelated task");
        std::fs::remove_dir_all(root).unwrap();
    }
    for redo in [false, true] {
        let root = root();
        let old = root.join("old");
        let moved = root.join("moved");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("b.png"), b"B source").unwrap();
        let task = root.join("outside.png");
        std::fs::write(&task, b"outside task").unwrap();
        let mut s = Session::with_demo();
        let a = s.active().unwrap();
        let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
        set_source(&mut s, a, &task);
        set_source(&mut s, b, &old.join("b.png"));
        crate::cmd::browse::move_folder(&mut s, &old.to_string_lossy(), &moved.to_string_lossy()).unwrap();
        if redo {
            s.undo_step().unwrap();
        }
        let _lease = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
        if redo { s.redo_step() } else { s.undo_step() }.unwrap();
        assert_eq!(std::fs::read(&task).unwrap(), b"outside task");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    for redo in [false, true] {
        let root = root();
        let target = root.join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("b.png"), b"B source").unwrap();
        std::fs::write(target.join("a.png"), b"A source").unwrap();
        let alias = root.join("alias");
        let moved = root.join("renamed-alias");
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        let mut s = Session::with_demo();
        let a = s.active().unwrap();
        let b = s.catalog.photos().find(|p| p.id != a).unwrap().id;
        set_source(&mut s, b, &alias.join("b.png"));
        crate::cmd::browse::move_folder(&mut s, &alias.to_string_lossy(), &moved.to_string_lossy()).unwrap();
        if redo {
            s.undo_step().unwrap();
        }
        set_source(&mut s, a, &target.join("a.png"));
        let _lease = s.acquire_companion_lease(a, LocalRoundId::new().unwrap()).unwrap();
        if redo { s.redo_step() } else { s.undo_step() }.unwrap();
        assert_eq!(std::fs::read(target.join("a.png")).unwrap(), b"A source");
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn folder_traversal_cycles_are_bounded_without_locking_independent_entries() {
    let root = root();
    let folder = root.join("independent");
    std::fs::create_dir(&folder).unwrap();
    std::os::unix::fs::symlink("cycle-b", root.join("cycle-a")).unwrap();
    std::os::unix::fs::symlink("cycle-a", root.join("cycle-b")).unwrap();
    assert!(!folder_affects_path(&folder, &root.join("cycle-a/task.png")));
    // An expanding cycle does not repeat a spelling: the depth bound must also
    // stop it, independently of the visited-path set.
    std::os::unix::fs::symlink("expanding/again", root.join("expanding")).unwrap();
    assert!(!folder_affects_path(&folder, &root.join("expanding/task.png")));
    std::fs::remove_dir_all(root).unwrap();
}
