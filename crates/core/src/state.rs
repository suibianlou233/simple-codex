use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{AppEvent, Project, ProjectId, ProjectionError, Task, TaskId, Turn, TurnId};

/// Applies durable domain events to an in-memory read model.
pub trait StateProjection {
    fn apply(&mut self, event: &AppEvent) -> Result<(), ProjectionError>;
}

#[derive(Debug, Clone, Default)]
pub struct AppState {
    projects: BTreeMap<ProjectId, Project>,
    tasks: BTreeMap<TaskId, Task>,
    turns: BTreeMap<TurnId, Turn>,
}

/// Serializable state for the desktop task list. The maps remain an internal
/// implementation detail so the JSON shape stays language-neutral.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StateSnapshot {
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub turns: Vec<Turn>,
}

impl AppState {
    #[must_use]
    pub fn project(&self, id: ProjectId) -> Option<&Project> {
        self.projects.get(&id)
    }

    #[must_use]
    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks.get(&id)
    }

    #[must_use]
    pub fn turn(&self, id: TurnId) -> Option<&Turn> {
        self.turns.get(&id)
    }

    #[must_use]
    pub fn project_by_root(&self, root: &std::path::Path) -> Option<&Project> {
        self.projects.values().find(|project| project.root == root)
    }

    #[must_use]
    pub fn snapshot(&self) -> StateSnapshot {
        let mut projects: Vec<_> = self.projects.values().cloned().collect();
        projects.sort_by_key(|project| (project.opened_at_ms, project.id));
        let mut tasks: Vec<_> = self.tasks.values().cloned().collect();
        tasks.sort_by_key(|task| (task.created_at_ms, task.id));
        let mut turns: Vec<_> = self.turns.values().cloned().collect();
        turns.sort_by_key(|turn| (turn.started_at_ms, turn.id));

        StateSnapshot {
            projects,
            tasks,
            turns,
        }
    }
}

impl StateProjection for AppState {
    fn apply(&mut self, event: &AppEvent) -> Result<(), ProjectionError> {
        match event {
            AppEvent::ProjectOpened { project } => {
                if let Some(existing) = self.projects.get(&project.id) {
                    if existing == project {
                        return Ok(());
                    }
                    return Err(ProjectionError::DuplicateProject(project.id));
                }
                self.projects.insert(project.id, project.clone());
            }
            AppEvent::TaskCreated { task } => {
                if !self.projects.contains_key(&task.project_id) {
                    return Err(ProjectionError::MissingProject(task.project_id));
                }
                if self.tasks.contains_key(&task.id) {
                    return Err(ProjectionError::DuplicateTask(task.id));
                }
                self.tasks.insert(task.id, task.clone());
            }
            AppEvent::TurnStarted { turn } => {
                if !self.tasks.contains_key(&turn.task_id) {
                    return Err(ProjectionError::MissingTask(turn.task_id));
                }
                if self.turns.contains_key(&turn.id) {
                    return Err(ProjectionError::DuplicateTurn(turn.id));
                }
                self.turns.insert(turn.id, turn.clone());
                if let Some(task) = self.tasks.get_mut(&turn.task_id) {
                    task.status = crate::TaskStatus::Running;
                }
            }
            AppEvent::TurnTransitioned { turn } => {
                let Some(existing) = self.turns.get(&turn.id) else {
                    return Err(ProjectionError::MissingTurn(turn.id));
                };
                if existing.task_id != turn.task_id {
                    return Err(ProjectionError::MissingTask(turn.task_id));
                }
                self.turns.insert(turn.id, turn.clone());
                if let Some(task) = self.tasks.get_mut(&turn.task_id) {
                    task.status = crate::TaskStatus::Running;
                }
            }
            AppEvent::TurnFinished { turn } => {
                let Some(existing) = self.turns.get(&turn.id) else {
                    return Err(ProjectionError::MissingTurn(turn.id));
                };
                if existing.task_id != turn.task_id {
                    return Err(ProjectionError::MissingTask(turn.task_id));
                }
                self.turns.insert(turn.id, turn.clone());
                if let Some(task) = self.tasks.get_mut(&turn.task_id) {
                    task.status = match turn.status {
                        crate::TurnStatus::Completed => crate::TaskStatus::Completed,
                        crate::TurnStatus::Failed => crate::TaskStatus::Failed,
                        crate::TurnStatus::Cancelled => crate::TaskStatus::Ready,
                        crate::TurnStatus::Running => crate::TaskStatus::Running,
                    };
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::PathBuf;

    use crate::{AppEvent, Project, ProjectId, ProjectionError, Task, TaskId, TaskStatus};

    use super::{AppState, StateProjection};

    #[test]
    fn task_cannot_be_projected_without_its_project() {
        let task = Task {
            id: TaskId::new(),
            project_id: ProjectId::new(),
            title: "Explain this project".to_owned(),
            status: TaskStatus::Ready,
            created_at_ms: 1,
        };
        let mut state = AppState::default();

        assert_eq!(
            state.apply(&AppEvent::TaskCreated { task: task.clone() }),
            Err(ProjectionError::MissingProject(task.project_id))
        );
    }

    #[test]
    fn events_rebuild_a_snapshot() {
        let project = Project {
            id: ProjectId::new(),
            root: PathBuf::from("C:\\work\\demo"),
            name: "demo".to_owned(),
            opened_at_ms: 1,
        };
        let task = Task {
            id: TaskId::new(),
            project_id: project.id,
            title: "Read the project".to_owned(),
            status: TaskStatus::Ready,
            created_at_ms: 2,
        };
        let mut state = AppState::default();

        state
            .apply(&AppEvent::ProjectOpened {
                project: project.clone(),
            })
            .expect("project should apply");
        state
            .apply(&AppEvent::TaskCreated { task: task.clone() })
            .expect("task should apply");

        let snapshot = state.snapshot();
        assert_eq!(snapshot.projects, vec![project]);
        assert_eq!(snapshot.tasks, vec![task]);
    }
}
