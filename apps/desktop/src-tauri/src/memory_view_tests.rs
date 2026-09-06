use super::*;

#[test]
fn persisted_task_selects_only_its_project_and_history_memory_after_restart() {
    use super::super::{CreateTaskInput, PermissionLevel};
    use crate::secrets::MemorySecretStore;
    use std::sync::Arc;
    let temp = tempfile::tempdir().expect("fixture");
    let database = temp.path().join("state.db");
    let home = temp.path().join("native");
    let mut runtime =
        DesktopRuntime::open(&database, Arc::new(MemorySecretStore::new())).expect("runtime");
    let mut tasks = Vec::new();
    for name in ["项目 A", "项目 B"] {
        let root = temp.path().join(name);
        std::fs::create_dir(&root).expect("project");
        let snapshot = runtime.open_project(root.clone()).expect("open");
        let canonical = std::fs::canonicalize(&root).expect("canonical");
        let project = snapshot
            .projects
            .iter()
            .find(|p| p.path == user_visible_path(&canonical))
            .expect("project")
            .id
            .clone();
        let snapshot = runtime
            .create_task(CreateTaskInput {
                project_id: project.clone(),
                title: name.into(),
                goal: "fixture".into(),
                permission_level: PermissionLevel::Approval,
            })
            .expect("task");
        tasks.push(
            snapshot
                .tasks
                .iter()
                .find(|t| t.project_id == project)
                .expect("task")
                .id
                .clone(),
        );
    }
    // A pre-existing history registration must remain independent of the new default.
    let legacy = hash_bytes(b"legacy-viewer-fixture");
    runtime
        .storage
        .resolve_codex_history_home(
            &tasks[0],
            &home,
            &legacy,
            local_agent_model::CodexHistoryLayout::legacy_memory().storage_probe(),
        )
        .expect("legacy registration");
    let mut paths = Vec::new();
    for (index, task) in tasks.iter().enumerate() {
        let inspection =
            MemoryInspection::prepare(&mut runtime, &home, task.clone()).expect("inspection");
        assert_eq!(inspection.view.legacy_history, index == 0);
        let path = inspection.artifact_home.clone();
        assert!(
            inspection
                .read()
                .documents
                .iter()
                .all(|entry| entry.status == "missing")
        );
        assert!(
            !path.exists(),
            "inspection must not create memory artifacts"
        );
        std::fs::create_dir_all(&path).expect("artifact fixture");
        std::fs::write(path.join("MEMORY.md"), format!("MEMORY_FOR_{index}")).expect("content");
        paths.push(path);
    }
    assert_ne!(paths[0], paths[1]);
    assert!(
        paths[0]
            .components()
            .any(|component| component.as_os_str() == std::ffi::OsStr::new(&legacy))
    );
    assert!(MemoryInspection::prepare(&mut runtime, &home, "../other".into()).is_err());
    assert!(
        MemoryInspection::prepare(
            &mut runtime,
            &home,
            local_agent_core::TaskId::new().to_string()
        )
        .is_err()
    );
    drop(runtime);
    let mut restored =
        DesktopRuntime::open(&database, Arc::new(MemorySecretStore::new())).expect("restart");
    for (index, task) in tasks.iter().enumerate() {
        let inspection = MemoryInspection::prepare(&mut restored, &home, task.clone())
            .expect("restored inspection");
        assert_eq!(inspection.artifact_home, paths[index]);
        let view = inspection.read();
        assert_eq!(view.task_id, *task);
        assert_eq!(view.legacy_history, index == 0);
        assert_eq!(
            view.documents[1].content.as_deref(),
            Some(format!("MEMORY_FOR_{index}").as_str())
        );
        assert_eq!(
            std::fs::read_to_string(paths[index].join("MEMORY.md")).expect("unchanged"),
            format!("MEMORY_FOR_{index}")
        );
    }
}

#[test]
fn viewer_distinguishes_missing_empty_invalid_and_oversized_without_writes() {
    let temp = tempfile::tempdir().expect("fixture");
    let root = temp.path().join("memories");
    let missing = read_documents(&root);
    assert!(missing.iter().all(|entry| entry.status == "missing"));
    assert!(!root.exists());
    std::fs::create_dir(&root).expect("root");
    std::fs::write(root.join("memory_summary.md"), "").expect("empty summary");
    std::fs::write(
        root.join("MEMORY.md"),
        "用户自己的记忆\n<script>do not execute</script>",
    )
    .expect("memory");
    std::fs::write(root.join("raw_memories.md"), [0xff, 0xfe]).expect("invalid UTF8");
    let view = read_documents(&root);
    assert_eq!(view[0].status, "ready");
    assert_eq!(view[0].content.as_deref(), Some(""));
    let original = std::fs::read_to_string(root.join("MEMORY.md")).expect("memory");
    assert_eq!(view[1].content.as_deref(), Some(original.as_str()));
    assert_eq!(
        view[1].hash.as_deref(),
        Some(local_agent_tools::hash_bytes(original.as_bytes()).as_str())
    );
    assert_eq!(view[2].status, "unreadable");
    assert!(view[2].content.is_none());
    let large = vec![b'x'; MAX_FILE_BYTES as usize + 1];
    std::fs::write(root.join("MEMORY.md"), &large).expect("large fixture");
    assert_eq!(read_documents(&root)[1].status, "tooLarge");
    assert_eq!(
        std::fs::read(root.join("MEMORY.md")).expect("unchanged"),
        large
    );
}

#[test]
fn viewer_does_not_read_unlisted_files_or_directories_as_text() {
    let temp = tempfile::tempdir().expect("fixture");
    std::fs::create_dir(temp.path().join("MEMORY.md")).expect("non-file");
    std::fs::write(temp.path().join("private.env"), "PRIVATE_FIXTURE").expect("not an artifact");
    let view = read_documents(temp.path());
    assert_eq!(view.len(), 3);
    assert_eq!(view[1].status, "unsafe");
    assert!(view.iter().all(|entry| entry.content.is_none()));
}

#[cfg(windows)]
#[test]
fn viewer_rejects_linked_artifact_directory() {
    let temp = tempfile::tempdir().expect("fixture");
    let target = temp.path().join("target");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&target).expect("target");
    std::fs::write(target.join("MEMORY.md"), "FOREIGN_FIXTURE").expect("fixture");
    assert!(
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&alias)
            .arg(&target)
            .output()
            .expect("junction")
            .status
            .success()
    );
    assert!(
        read_documents(&alias)
            .iter()
            .all(|entry| entry.status == "unsafe" && entry.content.is_none())
    );
}
