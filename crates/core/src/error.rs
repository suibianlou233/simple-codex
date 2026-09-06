use std::path::PathBuf;

use thiserror::Error;

use crate::{ProjectId, TaskId, TurnId, TurnPhase};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("项目路径不是已存在的文件夹：{0}")]
    InvalidProjectPath(PathBuf),
    #[error("项目不存在：{0}")]
    ProjectNotFound(ProjectId),
    #[error("任务不存在：{0}")]
    TaskNotFound(TaskId),
    #[error("回复轮次不存在：{0}")]
    TurnNotFound(TurnId),
    #[error("任务已有正在运行的回复：{0}")]
    TaskAlreadyRunning(TaskId),
    #[error("回复轮次已经结束：{0}")]
    TurnAlreadyFinished(TurnId),
    #[error("回复轮次状态不能从 {from:?} 转换到 {to:?}")]
    InvalidTurnTransition { from: TurnPhase, to: TurnPhase },
    #[error("任务标题不能为空")]
    EmptyTaskTitle,
    #[error("任务标题有 {actual_chars} 个字符，最多允许 {max_chars} 个")]
    TaskTitleTooLong {
        max_chars: usize,
        actual_chars: usize,
    },
    #[error("系统时间早于 Unix 纪元")]
    InvalidSystemTime,
    #[error(transparent)]
    Projection(#[from] ProjectionError),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProjectionError {
    #[error("项目已存在：{0}")]
    DuplicateProject(ProjectId),
    #[error("任务已存在：{0}")]
    DuplicateTask(TaskId),
    #[error("回复轮次已存在：{0}")]
    DuplicateTurn(TurnId),
    #[error("任务引用了不存在的项目：{0}")]
    MissingProject(ProjectId),
    #[error("回复轮次引用了不存在的任务：{0}")]
    MissingTask(TaskId),
    #[error("要结束的回复轮次不存在：{0}")]
    MissingTurn(TurnId),
}
