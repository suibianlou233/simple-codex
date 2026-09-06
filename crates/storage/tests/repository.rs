use std::sync::Arc;
use std::sync::Barrier;
use std::thread;

use local_agent_storage::NewEvent;
use local_agent_storage::NewModelProfile;
use local_agent_storage::NewProject;
use local_agent_storage::NewTask;
use local_agent_storage::Storage;
use local_agent_storage::StorageError;
use serde_json::json;
use tempfile::tempdir;

fn task(task_id: &str, created_at_ms: i64) -> NewTask {
    NewTask {
        task_id: task_id.to_owned(),
        project_id: "project-1".to_owned(),
        title: format!("Task {task_id}"),
        created_at_ms,
    }
}

fn create_project(storage: &mut Storage) {
    storage
        .create_project(NewProject {
            project_id: "project-1".to_owned(),
            root: "C:/work/project".to_owned(),
            name: "Project".to_owned(),
        })
        .expect("create project");
}

fn event(event_id: &str, task_id: &str, created_at_ms: i64) -> NewEvent {
    NewEvent {
        event_id: event_id.to_owned(),
        task_id: task_id.to_owned(),
        turn_id: Some("turn-1".to_owned()),
        event_type: "UserMessage".to_owned(),
        payload: json!({ "text": event_id }),
        created_at_ms,
    }
}

fn model_profile(profile_id: &str, name: &str, is_default: bool) -> NewModelProfile {
    NewModelProfile {
        profile_id: profile_id.to_owned(),
        name: name.to_owned(),
        base_url: "https://models.example/v1".to_owned(),
        model: "fast-model".to_owned(),
        dialect: "qwen".to_owned(),
        credential_ref: format!("model-profile:{profile_id}"),
        max_output_tokens: Some(4096),
        context_window_tokens: Some(32_768),
        timeout_ms: 60_000,
        is_default,
        created_at_ms: 10,
        updated_at_ms: 10,
    }
}

#[test]
fn model_profiles_persist_without_secret_values_and_keep_one_default() {
    let directory = tempdir().expect("temporary directory");
    let database = directory.path().join("state.sqlite3");
    {
        let mut storage = Storage::open(&database).expect("open database");
        storage
            .upsert_model_profile(model_profile("deepseek", "DeepSeek", true))
            .expect("save first profile");
        storage
            .upsert_model_profile(model_profile("qwen", "千问", true))
            .expect("save second profile");
    }

    let storage = Storage::open(&database).expect("reopen database");
    let profiles = storage.list_model_profiles().expect("list profiles");
    assert_eq!(profiles.len(), 2);
    assert!(
        profiles
            .iter()
            .all(|profile| profile.context_window_tokens == Some(32_768))
    );
    assert_eq!(
        profiles.iter().filter(|profile| profile.is_default).count(),
        1
    );
    assert_eq!(
        profiles
            .iter()
            .find(|profile| profile.is_default)
            .expect("one profile should be default")
            .profile_id,
        "qwen"
    );
    let database_bytes = std::fs::read(database).expect("read database bytes");
    assert!(!String::from_utf8_lossy(&database_bytes).contains("api-key-secret"));
}

#[test]
fn model_profile_context_window_is_optional_and_range_checked() {
    let mut storage = Storage::open_in_memory().expect("open database");
    let mut profile = model_profile("local", "Local", true);
    profile.context_window_tokens = None;
    let saved = storage
        .upsert_model_profile(profile.clone())
        .expect("optional context window should save");
    assert_eq!(saved.context_window_tokens, None);

    profile.context_window_tokens = Some(16_383);
    assert!(matches!(
        storage.upsert_model_profile(profile),
        Err(StorageError::InvalidField {
            field: "context_window_tokens",
            ..
        })
    ));
}

#[test]
fn grouped_events_are_atomic_and_keep_input_order() {
    let mut storage = Storage::open_in_memory().expect("open database");
    create_project(&mut storage);
    storage
        .create_task(task("task-1", 10))
        .expect("create task");

    let records = storage
        .append_events(vec![
            event("turn-start", "task-1", 20),
            event("user-message", "task-1", 21),
        ])
        .expect("append event group");
    assert_eq!(
        records
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        [1, 2]
    );

    let failed = storage.append_events(vec![
        event("will-roll-back", "task-1", 22),
        event("turn-start", "task-1", 23),
    ]);
    assert!(matches!(failed, Err(StorageError::DuplicateEvent(id)) if id == "turn-start"));
    let events = storage.load_events("task-1").expect("load events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].event_id, "user-message");
}

#[test]
fn task_list_and_event_history_survive_restart() {
    let directory = tempdir().expect("temporary directory");
    let database = directory.path().join("state/app.sqlite3");

    {
        let mut storage = Storage::open(&database).expect("open database");
        create_project(&mut storage);
        storage.create_task(task("older", 10)).expect("older task");
        storage.create_task(task("newer", 20)).expect("newer task");
        storage
            .append_event(event("event-1", "older", 30))
            .expect("append event");
    }

    let storage = Storage::open(&database).expect("reopen database");
    assert_eq!(storage.list_projects().expect("list projects").len(), 1);
    let tasks = storage.list_tasks().expect("list tasks");
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.task_id.as_str())
            .collect::<Vec<_>>(),
        ["older", "newer"]
    );

    let snapshot = storage
        .load_task("older")
        .expect("load task")
        .expect("task exists");
    assert_eq!(snapshot.task.last_sequence, 1);
    assert_eq!(snapshot.events.len(), 1);
    assert_eq!(snapshot.events[0].sequence, 1);
    assert_eq!(snapshot.events[0].payload, json!({ "text": "event-1" }));
}

#[test]
fn task_requires_a_persisted_project() {
    let mut storage = Storage::open_in_memory().expect("open database");

    let result = storage.create_task(task("task-1", 10));

    assert!(matches!(
        result,
        Err(StorageError::ProjectNotFound(id)) if id == "project-1"
    ));
    assert!(storage.list_tasks().expect("list tasks").is_empty());
}

#[test]
fn concurrent_ensure_project_keeps_one_row_for_the_same_root() {
    const WRITERS: usize = 2;

    let directory = tempdir().expect("temporary directory");
    let database = directory.path().join("state.sqlite3");
    drop(Storage::open(&database).expect("initialize database"));

    let barrier = Arc::new(Barrier::new(WRITERS));
    let handles = (0..WRITERS)
        .map(|index| {
            let barrier = Arc::clone(&barrier);
            let database = database.clone();
            thread::spawn(move || {
                let mut storage = Storage::open(database).expect("open writer database");
                barrier.wait();
                storage
                    .ensure_project(NewProject {
                        project_id: format!("project-{index}"),
                        root: "C:/work/canonical-project".to_owned(),
                        name: format!("Project {index}"),
                    })
                    .expect("ensure project")
            })
        })
        .collect::<Vec<_>>();

    let records = handles
        .into_iter()
        .map(|handle| handle.join().expect("writer thread"))
        .collect::<Vec<_>>();
    assert_eq!(records[0], records[1]);

    let storage = Storage::open(database).expect("reopen database");
    let projects = storage.list_projects().expect("list projects");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].root, "C:/work/canonical-project");
}

#[test]
fn task_and_initial_events_are_created_atomically_in_input_order() {
    let mut storage = Storage::open_in_memory().expect("open database");
    create_project(&mut storage);
    let events = vec![
        event("event-b", "task-1", 13),
        event("event-a", "task-1", 12),
        event("event-c", "task-1", 14),
    ];

    let snapshot = storage
        .create_task_with_events(task("task-1", 10), events)
        .expect("create task and events");

    assert_eq!(snapshot.task.last_sequence, 3);
    assert_eq!(snapshot.task.updated_at_ms, 14);
    assert_eq!(
        snapshot
            .events
            .iter()
            .map(|event| (event.event_id.as_str(), event.sequence))
            .collect::<Vec<_>>(),
        [("event-b", 1), ("event-a", 2), ("event-c", 3)]
    );
    assert_eq!(
        storage
            .load_events("task-1")
            .expect("load events")
            .into_iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
}

#[test]
fn initial_event_failure_rolls_back_task_and_prior_events() {
    let mut storage = Storage::open_in_memory().expect("open database");
    create_project(&mut storage);
    let events = vec![
        event("duplicate", "task-1", 11),
        event("duplicate", "task-1", 12),
    ];

    let result = storage.create_task_with_events(task("task-1", 10), events);

    assert!(matches!(
        result,
        Err(StorageError::DuplicateEvent(id)) if id == "duplicate"
    ));
    assert!(storage.get_task("task-1").expect("get task").is_none());
    assert!(storage.list_tasks().expect("list tasks").is_empty());
}

#[test]
fn event_sequences_are_strictly_increasing() {
    let mut storage = Storage::open_in_memory().expect("open database");
    create_project(&mut storage);
    storage
        .create_task(task("task-1", 10))
        .expect("create task");

    let first = storage
        .append_event(event("event-1", "task-1", 11))
        .expect("first event");
    let second = storage
        .append_event(event("event-2", "task-1", 12))
        .expect("second event");

    assert_eq!(first.sequence, 1);
    assert_eq!(second.sequence, 2);
    assert_eq!(
        storage
            .load_events("task-1")
            .expect("load events")
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        [1, 2]
    );
}

#[test]
fn duplicate_event_does_not_consume_a_sequence() {
    let mut storage = Storage::open_in_memory().expect("open database");
    create_project(&mut storage);
    storage
        .create_task(task("task-1", 10))
        .expect("create task");
    storage
        .append_event(event("event-1", "task-1", 11))
        .expect("first event");

    let duplicate = storage.append_event(event("event-1", "task-1", 12));
    assert!(matches!(duplicate, Err(StorageError::DuplicateEvent(id)) if id == "event-1"));

    let second = storage
        .append_event(event("event-2", "task-1", 13))
        .expect("second event");
    assert_eq!(second.sequence, 2);
}

#[test]
fn event_for_unknown_task_is_rejected() {
    let mut storage = Storage::open_in_memory().expect("open database");
    let result = storage.append_event(event("event-1", "missing", 10));

    assert!(matches!(result, Err(StorageError::TaskNotFound(id)) if id == "missing"));
}

#[test]
fn concurrent_connections_allocate_unique_contiguous_sequences() {
    const WRITERS: usize = 8;

    let directory = tempdir().expect("temporary directory");
    let database = directory.path().join("state.sqlite3");
    let mut initial = Storage::open(&database).expect("open database");
    create_project(&mut initial);
    initial
        .create_task(task("task-1", 10))
        .expect("create task");
    drop(initial);

    let barrier = Arc::new(Barrier::new(WRITERS));
    let handles = (0..WRITERS)
        .map(|index| {
            let barrier = Arc::clone(&barrier);
            let database = database.clone();
            thread::spawn(move || {
                let mut storage = Storage::open(database).expect("open writer database");
                barrier.wait();
                storage
                    .append_event(event(
                        &format!("event-{index}"),
                        "task-1",
                        20 + index as i64,
                    ))
                    .expect("append event")
                    .sequence
            })
        })
        .collect::<Vec<_>>();

    let mut assigned = handles
        .into_iter()
        .map(|handle| handle.join().expect("writer thread"))
        .collect::<Vec<_>>();
    assigned.sort_unstable();
    assert_eq!(assigned, (1..=WRITERS as i64).collect::<Vec<_>>());

    let storage = Storage::open(database).expect("reopen database");
    let sequences = storage
        .load_events("task-1")
        .expect("load events")
        .into_iter()
        .map(|event| event.sequence)
        .collect::<Vec<_>>();
    assert_eq!(sequences, (1..=WRITERS as i64).collect::<Vec<_>>());
}
