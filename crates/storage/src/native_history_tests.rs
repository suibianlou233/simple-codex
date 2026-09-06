use super::*;
const TEST_LAYOUT: (&str, &str) = (
    "state_5.sqlite",
    "SELECT EXISTS(SELECT 1 FROM threads WHERE id = ?1)",
);
use crate::{NewCodexThreadBinding, NewModelProfile, NewProject, NewTask};

fn seed_task(storage: &mut Storage, native_thread: Option<&str>) {
    storage
        .create_project(NewProject {
            project_id: "p".into(),
            root: "fixture-project".into(),
            name: "project".into(),
        })
        .expect("project");
    storage
        .create_task(NewTask {
            task_id: "t".into(),
            project_id: "p".into(),
            title: "task".into(),
            created_at_ms: 1,
        })
        .expect("task");
    if let Some(id) = native_thread {
        storage
            .upsert_model_profile(NewModelProfile {
                profile_id: "model".into(),
                name: "model".into(),
                base_url: "http://127.0.0.1:1".into(),
                model: "fixture".into(),
                dialect: "standard".into(),
                credential_ref: "fixture-only".into(),
                max_output_tokens: None,
                context_window_tokens: None,
                timeout_ms: 1000,
                is_default: true,
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("profile");
        storage
            .bind_codex_thread(NewCodexThreadBinding {
                task_id: "t".into(),
                codex_thread_id: id.into(),
                model_profile_id: "model".into(),
                created_at_ms: 2,
            })
            .expect("binding");
    }
}

fn seed_native(root: &Path, home: &str, thread: &str) -> Vec<u8> {
    let path = root.join(home);
    std::fs::create_dir_all(&path).expect("home");
    let database = path.join("state_5.sqlite");
    let native = Connection::open(&database).expect("native fixture");
    native
        .execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, preserved TEXT NOT NULL);")
        .expect("native schema subset");
    native
        .execute(
            "INSERT INTO threads VALUES (?1, 'untouched metadata')",
            [thread],
        )
        .expect("row");
    drop(native);
    std::fs::write(
        path.join("thread_history_1.sqlite"),
        b"opaque paginated history sentinel",
    )
    .expect("history");
    std::fs::read(database).expect("before bytes")
}

#[test]
fn existing_history_location_survives_restart_and_changed_default_without_copying_history() {
    let temp = tempfile::tempdir().expect("fixture");
    let root = temp.path().join("kernels");
    let home = "a".repeat(64);
    let other = "b".repeat(64);
    let before = seed_native(&root, &home, "native-thread");
    let product_db = temp.path().join("simple.sqlite");
    let mut storage = Storage::open(&product_db).expect("product");
    seed_task(&mut storage, Some("native-thread"));
    assert_eq!(
        storage
            .resolve_codex_history_home("t", &root, &other, TEST_LAYOUT)
            .expect("locate"),
        home
    );
    drop(storage);
    let mut reopened = Storage::open(product_db).expect("reopen");
    assert_eq!(
        reopened
            .resolve_codex_history_home("t", &root, &"c".repeat(64), TEST_LAYOUT)
            .expect("stable location"),
        home
    );
    assert_eq!(
        std::fs::read(root.join(&home).join("state_5.sqlite")).expect("metadata"),
        before
    );
    assert_eq!(
        std::fs::read(root.join(&home).join("thread_history_1.sqlite")).expect("pages"),
        b"opaque paginated history sentinel"
    );
    assert!(!root.join(other).exists());
}

#[test]
fn ambiguous_missing_and_replaced_history_never_rebind_a_task() {
    let temp = tempfile::tempdir().expect("fixture");
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let default = "c".repeat(64);
    let mut storage = Storage::open_in_memory().expect("product");
    seed_task(&mut storage, Some("native-thread"));
    assert!(
        storage
            .resolve_codex_history_home("t", temp.path(), &default, TEST_LAYOUT)
            .is_err()
    );
    seed_native(temp.path(), &a, "native-thread");
    seed_native(temp.path(), &b, "native-thread");
    assert!(
        storage
            .resolve_codex_history_home("t", temp.path(), &default, TEST_LAYOUT)
            .is_err()
    );
    let duplicate = temp.path().join(&b).join("state_5.sqlite");
    std::fs::rename(&duplicate, duplicate.with_extension("preserved-copy"))
        .expect("retain duplicate outside discovery");
    assert_eq!(
        storage
            .resolve_codex_history_home("t", temp.path(), &default, TEST_LAYOUT)
            .expect("unique"),
        a
    );
    let original = temp.path().join(&a).join("state_5.sqlite");
    std::fs::rename(&original, original.with_extension("preserved-original"))
        .expect("simulate unavailable history");
    seed_native(temp.path(), &b, "native-thread");
    assert!(
        storage
            .resolve_codex_history_home("t", temp.path(), &default, TEST_LAYOUT)
            .is_err()
    );
    assert!(!temp.path().join(default).exists());
}

#[test]
fn fresh_task_binding_is_stable_and_rejects_path_components() {
    let mut storage = Storage::open_in_memory().expect("product");
    seed_task(&mut storage, None);
    let temp = tempfile::tempdir().expect("root");
    assert!(
        storage
            .resolve_codex_history_home("t", temp.path(), "../escape", TEST_LAYOUT)
            .is_err()
    );
    let first = "a".repeat(64);
    assert_eq!(
        storage
            .resolve_codex_history_home("t", temp.path(), &first, TEST_LAYOUT)
            .expect("new binding"),
        first
    );
    assert_eq!(
        storage
            .resolve_codex_history_home("t", temp.path(), &"b".repeat(64), TEST_LAYOUT)
            .expect("retry"),
        first
    );
    assert_eq!(std::fs::read_dir(temp.path()).expect("entries").count(), 0);
}

#[test]
fn unreadable_database_and_excessive_discovery_are_not_treated_as_missing_history() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = "a".repeat(64);
    let database = temp.path().join(&home).join("state_5.sqlite");
    std::fs::create_dir_all(database.parent().expect("parent")).expect("home");
    std::fs::write(&database, b"not a native SQLite database").expect("corrupt fixture");
    assert!(find_thread_home(temp.path(), "native", TEST_LAYOUT).is_err());
    assert_eq!(
        std::fs::read(&database).expect("original"),
        b"not a native SQLite database"
    );
    let many = temp.path().join("many");
    std::fs::create_dir_all(&many).expect("many root");
    for index in 0..513 {
        std::fs::create_dir(many.join(format!("{index:064x}"))).expect("candidate");
    }
    assert!(matches!(
        find_thread_home(&many, "native", TEST_LAYOUT),
        Err(StorageError::NativeHistory(_))
    ));
}

#[cfg(windows)]
#[test]
fn discovery_rejects_junctions_to_other_history_directories() {
    use std::os::windows::process::CommandExt;
    let temp = tempfile::tempdir().expect("fixture");
    let outside = temp.path().join("outside");
    let root = temp.path().join("kernels");
    std::fs::create_dir_all(&outside).expect("outside fixture");
    std::fs::create_dir_all(&root).expect("managed root");
    let junction = root.join("a".repeat(64));
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(junction)
        .arg(&outside)
        .creation_flags(0x0800_0000)
        .output()
        .expect("create test junction");
    assert!(
        output.status.success(),
        "junction fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(matches!(
        find_thread_home(&root, "native", TEST_LAYOUT),
        Err(StorageError::NativeHistory(_))
    ));
    assert!(outside.exists());
}

#[test]
fn version_supplied_history_layout_and_invalid_filename() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = "d".repeat(64);
    let root = temp.path().join("native");
    let folder = root.join(&home);
    std::fs::create_dir_all(&folder).expect("directory");
    let db = Connection::open(folder.join("different.sqlite")).expect("native");
    db.execute_batch(
        "CREATE TABLE sessions(token TEXT); INSERT INTO sessions VALUES ('thread-next');",
    )
    .expect("schema");
    drop(db);
    let mut storage = Storage::open(temp.path().join("simple.sqlite")).expect("product");
    seed_task(&mut storage, Some("thread-next"));
    let layout = (
        "different.sqlite",
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE token = ?1)",
    );
    assert!(
        storage
            .resolve_codex_history_home("t", &root, &home, ("../escape", layout.1))
            .is_err()
    );
    assert_eq!(
        storage
            .resolve_codex_history_home("t", &root, &home, layout)
            .expect("version layout"),
        home
    );
    assert!(!folder.join("state_5.sqlite").exists());
}
