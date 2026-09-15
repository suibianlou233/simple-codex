use local_agent_storage::{Storage, NewProject, NewTask};

#[test]
fn archive_restore_and_delete_survive_reopening_without_touching_other_tasks() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("test.db");
    let mut storage = Storage::open(&path).unwrap();
    storage.create_project(NewProject { project_id:"p".into(), root:"C:/fixture".into(), name:"fixture".into() }).unwrap();
    for id in ["a", "b"] {
        storage.create_task(NewTask { task_id:id.into(), project_id:"p".into(), title:id.into(), created_at_ms:1 }).unwrap();
    }
    storage.set_task_archived("a", true, "archive".into()).unwrap();
    drop(storage);
    let mut storage = Storage::open(&path).unwrap();
    assert!(storage.archived_task_ids().unwrap().contains("a"));
    storage.set_task_archived("a", false, "restore".into()).unwrap();
    assert!(storage.archived_task_ids().unwrap().is_empty());
    storage.delete_task_records("a").unwrap();
    drop(storage);
    let storage = Storage::open(&path).unwrap();
    assert!(storage.get_task("a").unwrap().is_none());
    assert!(storage.get_task("b").unwrap().is_some());
    assert!(storage.archived_task_ids().unwrap().is_empty());
}
