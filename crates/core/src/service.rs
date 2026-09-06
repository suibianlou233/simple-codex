use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    AppCommand, AppEvent, AppState, CoreError, MAX_TASK_TITLE_CHARS, Project, ProjectId,
    StateProjection, StateSnapshot, Task, TaskId, TaskStatus, Turn, TurnId, TurnStatus,
};

/// Phase-0 command decision service backed by an in-memory projection.
///
/// Call [`Self::decide`], durably store the returned event, and only then call
/// [`Self::apply`]. Keeping those steps separate prevents a failed database
/// write from advancing the in-memory state.
#[derive(Debug, Clone, Default)]
pub struct InMemoryTaskService {
    state: AppState,
}

impl InMemoryTaskService {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_events<'a>(
        events: impl IntoIterator<Item = &'a AppEvent>,
    ) -> Result<Self, CoreError> {
        let mut service = Self::new();
        for event in events {
            service.apply(event)?;
        }
        Ok(service)
    }

    pub fn decide(&self, command: AppCommand) -> Result<AppEvent, CoreError> {
        let event = match command {
            AppCommand::OpenProject { root } => self.open_project(&root)?,
            AppCommand::CreateTask { project_id, title } => self.create_task(project_id, &title)?,
            AppCommand::StartTurn { task_id } => self.start_turn(task_id)?,
            AppCommand::TransitionTurn { turn_id, phase } => {
                self.transition_turn(turn_id, phase)?
            }
            AppCommand::FinishTurn { turn_id, status } => self.finish_turn(turn_id, status)?,
        };
        Ok(event)
    }

    pub fn apply(&mut self, event: &AppEvent) -> Result<(), CoreError> {
        self.state.apply(event).map_err(CoreError::from)
    }

    #[must_use]
    pub fn snapshot(&self) -> StateSnapshot {
        self.state.snapshot()
    }

    fn open_project(&self, root: &Path) -> Result<AppEvent, CoreError> {
        let canonical_root = canonical_project_root(root)?;
        if let Some(project) = self.state.project_by_root(&canonical_root) {
            return Ok(AppEvent::ProjectOpened {
                project: project.clone(),
            });
        }

        let name = canonical_root
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("project")
            .to_owned();
        Ok(AppEvent::ProjectOpened {
            project: Project {
                id: ProjectId::new(),
                root: canonical_root,
                name,
                opened_at_ms: now_ms()?,
            },
        })
    }

    fn create_task(&self, project_id: ProjectId, title: &str) -> Result<AppEvent, CoreError> {
        if self.state.project(project_id).is_none() {
            return Err(CoreError::ProjectNotFound(project_id));
        }
        let title = title.trim();
        if title.is_empty() {
            return Err(CoreError::EmptyTaskTitle);
        }
        let title_chars = title.chars().count();
        if title_chars > MAX_TASK_TITLE_CHARS {
            return Err(CoreError::TaskTitleTooLong {
                max_chars: MAX_TASK_TITLE_CHARS,
                actual_chars: title_chars,
            });
        }

        Ok(AppEvent::TaskCreated {
            task: Task {
                id: TaskId::new(),
                project_id,
                title: title.to_owned(),
                status: TaskStatus::Ready,
                created_at_ms: now_ms()?,
            },
        })
    }

    fn start_turn(&self, task_id: TaskId) -> Result<AppEvent, CoreError> {
        if self.state.task(task_id).is_none() {
            return Err(CoreError::TaskNotFound(task_id));
        }
        if self
            .state
            .snapshot()
            .turns
            .iter()
            .any(|turn| turn.task_id == task_id && turn.status == TurnStatus::Running)
        {
            return Err(CoreError::TaskAlreadyRunning(task_id));
        }
        Ok(AppEvent::TurnStarted {
            turn: Turn {
                id: TurnId::new(),
                task_id,
                status: TurnStatus::Running,
                phase: crate::TurnPhase::Preparing,
                budget: crate::TurnBudget::default(),
                started_at_ms: now_ms()?,
                finished_at_ms: None,
            },
        })
    }

    fn transition_turn(
        &self,
        turn_id: TurnId,
        phase: crate::TurnPhase,
    ) -> Result<AppEvent, CoreError> {
        let existing = self
            .state
            .turn(turn_id)
            .ok_or(CoreError::TurnNotFound(turn_id))?;
        if existing.status != TurnStatus::Running {
            return Err(CoreError::TurnAlreadyFinished(turn_id));
        }
        if !existing.phase.can_transition_to(phase) {
            return Err(CoreError::InvalidTurnTransition {
                from: existing.phase,
                to: phase,
            });
        }
        let mut turn = existing.clone();
        turn.phase = phase;
        Ok(AppEvent::TurnTransitioned { turn })
    }

    fn finish_turn(&self, turn_id: TurnId, status: TurnStatus) -> Result<AppEvent, CoreError> {
        let existing = self
            .state
            .turn(turn_id)
            .ok_or(CoreError::TurnNotFound(turn_id))?;
        if existing.status != TurnStatus::Running {
            return Err(CoreError::TurnAlreadyFinished(turn_id));
        }
        let status = match status {
            TurnStatus::Running => TurnStatus::Failed,
            other => other,
        };
        Ok(AppEvent::TurnFinished {
            turn: Turn {
                id: existing.id,
                task_id: existing.task_id,
                status,
                phase: status.into(),
                budget: existing.budget,
                started_at_ms: existing.started_at_ms,
                finished_at_ms: Some(now_ms()?),
            },
        })
    }
}

fn canonical_project_root(root: &Path) -> Result<PathBuf, CoreError> {
    let canonical = std::fs::canonicalize(root)
        .map_err(|_| CoreError::InvalidProjectPath(root.to_path_buf()))?;
    if !canonical.is_dir() {
        return Err(CoreError::InvalidProjectPath(root.to_path_buf()));
    }
    Ok(canonical)
}

fn now_ms() -> Result<i64, CoreError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CoreError::InvalidSystemTime)?
        .as_millis();
    i64::try_from(millis).map_err(|_| CoreError::InvalidSystemTime)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::{AppCommand, AppEvent, CoreError, TaskStatus, TurnStatus};

    use super::InMemoryTaskService;

    fn unique_test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("local-agent-{label}-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn opening_a_project_and_creating_a_task_updates_state() {
        let root = unique_test_directory("create-task");
        fs::create_dir_all(&root).expect("test project should be created");
        let mut service = InMemoryTaskService::new();

        let opened = service
            .decide(AppCommand::OpenProject { root: root.clone() })
            .expect("project should open");
        service.apply(&opened).expect("project should apply");
        let AppEvent::ProjectOpened { project } = opened else {
            panic!("expected project event");
        };
        let created = service
            .decide(AppCommand::CreateTask {
                project_id: project.id,
                title: "  Build a local agent  ".to_owned(),
            })
            .expect("task should be created");
        service.apply(&created).expect("task should apply");

        let AppEvent::TaskCreated { task } = created else {
            panic!("expected task event");
        };
        assert_eq!(task.title, "Build a local agent");
        assert_eq!(service.snapshot().projects.len(), 1);
        assert_eq!(service.snapshot().tasks, vec![task]);

        fs::remove_dir_all(root).expect("test project should be removed");
    }

    #[test]
    fn reopening_the_same_path_is_idempotent() {
        let root = unique_test_directory("reopen");
        fs::create_dir_all(&root).expect("test project should be created");
        let mut service = InMemoryTaskService::new();

        let first = service
            .decide(AppCommand::OpenProject { root: root.clone() })
            .expect("project should open");
        service.apply(&first).expect("project should apply");
        let second = service
            .decide(AppCommand::OpenProject { root: root.clone() })
            .expect("project should reopen");
        service
            .apply(&second)
            .expect("reopened project should apply idempotently");

        assert_eq!(first, second);
        assert_eq!(service.snapshot().projects.len(), 1);

        fs::remove_dir_all(root).expect("test project should be removed");
    }

    #[test]
    fn invalid_project_is_rejected() {
        let service = InMemoryTaskService::new();
        let missing = unique_test_directory("missing");
        assert_eq!(
            service.decide(AppCommand::OpenProject {
                root: missing.clone()
            }),
            Err(CoreError::InvalidProjectPath(missing))
        );
    }

    #[test]
    fn empty_task_title_is_rejected() {
        let root = unique_test_directory("empty-title");
        fs::create_dir_all(&root).expect("test project should be created");
        let mut service = InMemoryTaskService::new();
        let opened = service
            .decide(AppCommand::OpenProject { root: root.clone() })
            .expect("project should open");
        service.apply(&opened).expect("project should apply");
        let AppEvent::ProjectOpened { project } = opened else {
            panic!("expected project event");
        };

        assert_eq!(
            service.decide(AppCommand::CreateTask {
                project_id: project.id,
                title: "  ".to_owned(),
            }),
            Err(CoreError::EmptyTaskTitle)
        );

        fs::remove_dir_all(root).expect("test project should be removed");
    }

    #[test]
    fn task_title_unicode_scalar_limit_is_enforced_after_trimming() {
        let root = unique_test_directory("title-limit");
        fs::create_dir_all(&root).expect("test project should be created");
        let mut service = InMemoryTaskService::new();
        let opened = service
            .decide(AppCommand::OpenProject { root: root.clone() })
            .expect("project should open");
        service.apply(&opened).expect("project should apply");
        let AppEvent::ProjectOpened { project } = opened else {
            panic!("expected project event");
        };

        let maximum = "界".repeat(crate::MAX_TASK_TITLE_CHARS);
        let accepted = service
            .decide(AppCommand::CreateTask {
                project_id: project.id,
                title: format!("  {maximum}  "),
            })
            .expect("an 80-character title should be accepted");
        let AppEvent::TaskCreated { task } = accepted else {
            panic!("expected task event");
        };
        assert_eq!(task.title.chars().count(), crate::MAX_TASK_TITLE_CHARS);

        let over_limit = "界".repeat(crate::MAX_TASK_TITLE_CHARS + 1);
        assert_eq!(
            service.decide(AppCommand::CreateTask {
                project_id: project.id,
                title: over_limit,
            }),
            Err(CoreError::TaskTitleTooLong {
                max_chars: crate::MAX_TASK_TITLE_CHARS,
                actual_chars: crate::MAX_TASK_TITLE_CHARS + 1,
            })
        );

        fs::remove_dir_all(root).expect("test project should be removed");
    }

    #[test]
    fn state_can_be_rebuilt_from_events() {
        let root = unique_test_directory("replay");
        fs::create_dir_all(&root).expect("test project should be created");
        let mut original = InMemoryTaskService::new();
        let opened = original
            .decide(AppCommand::OpenProject { root: root.clone() })
            .expect("project should open");
        original.apply(&opened).expect("project should apply");
        let AppEvent::ProjectOpened { project } = &opened else {
            panic!("expected project event");
        };
        let created = original
            .decide(AppCommand::CreateTask {
                project_id: project.id,
                title: "Persist me".to_owned(),
            })
            .expect("task should be created");
        original.apply(&created).expect("task should apply");

        let events = [&opened, &created];
        let restored =
            InMemoryTaskService::from_events(events).expect("events should rebuild the state");
        assert_eq!(restored.snapshot(), original.snapshot());

        fs::remove_dir_all(root).expect("test project should be removed");
    }

    #[test]
    fn turn_transitions_update_task_and_replay() {
        let root = unique_test_directory("turn");
        fs::create_dir_all(&root).expect("test project should be created");
        let mut service = InMemoryTaskService::new();
        let opened = service
            .decide(AppCommand::OpenProject { root: root.clone() })
            .expect("project should open");
        service.apply(&opened).expect("project should apply");
        let AppEvent::ProjectOpened { project } = &opened else {
            panic!("expected project event");
        };
        let created = service
            .decide(AppCommand::CreateTask {
                project_id: project.id,
                title: "Stream a response".to_owned(),
            })
            .expect("task should be created");
        service.apply(&created).expect("task should apply");
        let AppEvent::TaskCreated { task } = &created else {
            panic!("expected task event");
        };
        let started = service
            .decide(AppCommand::StartTurn { task_id: task.id })
            .expect("turn should start");
        service.apply(&started).expect("turn should apply");
        let AppEvent::TurnStarted { turn } = &started else {
            panic!("expected turn event");
        };
        assert_eq!(service.snapshot().tasks[0].status, TaskStatus::Running);

        let finished = service
            .decide(AppCommand::FinishTurn {
                turn_id: turn.id,
                status: TurnStatus::Completed,
            })
            .expect("turn should finish");
        service.apply(&finished).expect("finish should apply");
        assert_eq!(service.snapshot().tasks[0].status, TaskStatus::Completed);

        let events = [&opened, &created, &started, &finished];
        let restored = InMemoryTaskService::from_events(events).expect("turn events should replay");
        assert_eq!(restored.snapshot(), service.snapshot());
        fs::remove_dir_all(root).expect("test project should be removed");
    }
}
