use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{Project, ProjectId, Task, TaskId, Turn, TurnId, TurnPhase, TurnStatus};

/// Commands accepted by the phase-0 local core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum AppCommand {
    OpenProject {
        root: PathBuf,
    },
    CreateTask {
        project_id: ProjectId,
        title: String,
    },
    StartTurn {
        task_id: TaskId,
    },
    TransitionTurn {
        turn_id: TurnId,
        phase: TurnPhase,
    },
    FinishTurn {
        turn_id: TurnId,
        status: TurnStatus,
    },
}

/// Events emitted after commands have been accepted.
///
/// Persistence metadata such as event sequence belongs to the storage
/// envelope, not this desktop-facing payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum AppEvent {
    ProjectOpened { project: Project },
    TaskCreated { task: Task },
    TurnStarted { turn: Turn },
    TurnTransitioned { turn: Turn },
    TurnFinished { turn: Turn },
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::PathBuf;

    use super::AppCommand;

    #[test]
    fn command_uses_a_stable_tagged_shape() {
        let command = AppCommand::OpenProject {
            root: PathBuf::from("C:\\work\\demo"),
        };
        let value = serde_json::to_value(command).expect("command should serialize");

        assert_eq!(value["type"], "open_project");
        assert_eq!(value["payload"]["root"], "C:\\work\\demo");
    }
}
