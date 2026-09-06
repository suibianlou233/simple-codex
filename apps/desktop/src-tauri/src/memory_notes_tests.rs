use super::*;
use crate::runtime::memory_notes::SaveMemoryNotesInput;

fn save(
    runtime: &mut DesktopRuntime,
    prepared: &PreparedCodexTurn,
    revision: i64,
    content: &str,
) -> Result<crate::runtime::memory_notes::MemoryNotesView, DesktopError> {
    runtime.save_memory_notes(SaveMemoryNotesInput {
        task_id: prepared.task_id.to_string(),
        expected_revision: revision,
        content: content.into(),
    })
}

#[test]
fn manual_memory_survives_restart_rejects_stale_editors_and_clears_without_resurrection() {
    let (temp, mut runtime, prepared) = fixture();
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    assert_eq!(
        save(&mut runtime, &prepared, 0, "Correct project fact")
            .expect("save")
            .revision,
        1
    );
    assert!(matches!(
        save(&mut runtime, &prepared, 0, "stale"),
        Err(DesktopError::Storage(
            local_agent_storage::StorageError::MemoryNotesConflict
        ))
    ));
    drop(runtime);
    let mut runtime = DesktopRuntime::open(
        &temp.path().join("data.db"),
        Arc::new(MemorySecretStore::new()),
    )
    .expect("restart");
    assert_eq!(
        runtime
            .load_memory_notes(&prepared.task_id.to_string())
            .expect("loaded")
            .content,
        "Correct project fact"
    );
    assert_eq!(
        save(&mut runtime, &prepared, 1, "")
            .expect("delete")
            .revision,
        2
    );
    assert!(save(&mut runtime, &prepared, 1, "resurrect").is_err());
    assert!(
        runtime
            .load_memory_notes(&prepared.task_id.to_string())
            .expect("cleared")
            .content
            .is_empty()
    );
    let events = serde_json::to_string(
        &runtime
            .storage
            .load_events(&prepared.task_id.to_string())
            .expect("events"),
    )
    .expect("serialize");
    assert!(!events.contains("Correct project fact"));
    assert!(!events.contains("resurrect"));
}

#[test]
fn manual_memory_refuses_busy_and_oversize_without_losing_existing_content() {
    let (_temp, mut runtime, prepared) = fixture();
    assert!(matches!(
        save(&mut runtime, &prepared, 0, "busy"),
        Err(DesktopError::ProjectBusy)
    ));
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    save(&mut runtime, &prepared, 0, "keep").expect("save");
    assert!(save(&mut runtime, &prepared, 1, &"中".repeat(3000)).is_err());
    assert_eq!(
        runtime
            .load_memory_notes(&prepared.task_id.to_string())
            .expect("kept")
            .content,
        "keep"
    );
}

#[test]
fn manual_memory_is_user_input_and_obeys_the_task_memory_switch() {
    let (_temp, mut runtime, prepared) = fixture();
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    save(&mut runtime, &prepared, 0, "CORRECTED_FACT").expect("save");
    let input = runtime.codex_user_input(&prepared).expect("input");
    assert_eq!(input.len(), 2);
    assert_eq!(input[0]["type"], "text");
    assert!(
        input[0]["text"]
            .as_str()
            .expect("text")
            .contains("CORRECTED_FACT")
    );
    assert_eq!(input[1]["text"], prepared.content);
    assert!(input.iter().all(|item| item.get("role").is_none()));
    runtime.task_memory_enabled.insert(prepared.task_id, false);
    assert_eq!(
        runtime.codex_user_input(&prepared).expect("disabled"),
        vec![input[1].clone()]
    );
    assert_eq!(
        runtime
            .load_memory_notes(&prepared.task_id.to_string())
            .expect("not deleted")
            .content,
        "CORRECTED_FACT"
    );
}

#[test]
fn manual_memory_audit_failure_rolls_back_content_and_revision() {
    let (_temp, mut runtime, prepared) = fixture();
    let task = prepared.task_id.to_string();
    runtime
        .storage
        .save_project_memory_notes(&task, 0, "first".into(), "fixed-event".into(), 1)
        .expect("first");
    assert!(
        runtime
            .storage
            .save_project_memory_notes(&task, 1, "must rollback".into(), "fixed-event".into(), 2)
            .is_err()
    );
    let notes = runtime
        .storage
        .load_project_memory_notes(&task)
        .expect("transaction rolled back");
    assert_eq!(notes.content, "first");
    assert_eq!(notes.revision, 1);
}

#[test]
fn manual_memory_rejects_known_credentials_and_auth_lines_before_persistence() {
    let (_temp, mut runtime, prepared) = fixture();
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    runtime
        .secret_store
        .set(
            &prepared.profile.credential_ref,
            "FAKE_TEST_CREDENTIAL_1234",
        )
        .expect("fake credential");
    for content in [
        "FAKE_TEST_CREDENTIAL_1234",
        "Authorization: Bearer fixture",
        "api_key=fixture",
        "[REDACTED]\napi_key=fixture",
    ] {
        assert!(matches!(
            save(&mut runtime, &prepared, 0, content),
            Err(DesktopError::SensitiveMemoryNotes)
        ));
    }
    assert_eq!(
        runtime
            .load_memory_notes(&prepared.task_id.to_string())
            .expect("unchanged")
            .revision,
        0
    );
}

#[test]
fn manual_memory_compare_and_save_checks_another_database_connection() {
    let (temp, mut runtime, prepared) = fixture();
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    let task = prepared.task_id.to_string();
    let mut other = Storage::open(temp.path().join("data.db")).expect("independent connection");
    assert_eq!(
        other
            .load_project_memory_notes(&task)
            .expect("old view")
            .revision,
        0
    );
    save(&mut runtime, &prepared, 0, "winner").expect("first writer");
    assert!(matches!(
        other.save_project_memory_notes(
            &task,
            0,
            "stale overwrite".into(),
            "other-event".into(),
            1
        ),
        Err(local_agent_storage::StorageError::MemoryNotesConflict)
    ));
    assert_eq!(
        other
            .load_project_memory_notes(&task)
            .expect("latest")
            .content,
        "winner"
    );
}

#[test]
fn manual_memory_is_shared_by_project_tasks_not_model_profiles_or_other_projects() {
    let (temp, mut runtime, prepared) = fixture();
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    save(&mut runtime, &prepared, 0, "PROJECT_A_ONLY").expect("save");
    runtime
        .save_model_profile(SaveModelProfileInput {
            profile_id: None,
            name: "different model".into(),
            base_url: "http://127.0.0.1:8001/v1".into(),
            model: "second-model".into(),
            dialect: "standard".into(),
            api_key: None,
            max_output_tokens: Some(1024),
            context_window_tokens: Some(32768),
            timeout_ms: 30000,
            is_default: true,
        })
        .expect("another default model");
    for same in [true, false] {
        let root = if same {
            prepared.project_root.clone()
        } else {
            let root = temp.path().join("independent");
            fs::create_dir(&root).expect("root");
            root
        };
        runtime.open_project(root.clone()).expect("open");
        let root = fs::canonicalize(root).expect("canonical");
        let project_id = runtime
            .core
            .snapshot()
            .projects
            .into_iter()
            .find(|p| fs::canonicalize(&p.root).expect("root") == root)
            .expect("project")
            .id
            .to_string();
        let next = runtime
            .prepare_codex_new_chat(&StartChatInput {
                project_id,
                profile_id: None,
                content: "next".into(),
                permission_level: PermissionLevel::Approval,
            })
            .expect("next");
        assert_ne!(next.profile.profile_id, prepared.profile.profile_id);
        assert_eq!(
            serde_json::to_string(&runtime.codex_user_input(&next).expect("scoped"))
                .expect("json")
                .contains("PROJECT_A_ONLY"),
            same
        );
        if same {
            assert!(matches!(
                save(&mut runtime, &prepared, 1, "busy"),
                Err(DesktopError::ProjectBusy)
            ));
        }
        finish(&mut runtime, &next, ProjectedTurnStatus::Completed);
    }
}
