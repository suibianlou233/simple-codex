import { useState } from "react";
import type { DesktopSnapshot, ProjectSummary, TaskSummary } from "../bridge/types";
import type { ResolvedTheme, ThemePreference } from "../theme";
import { findTasks, relativeTaskTime } from "../app/navigation";
import { Icon, SimpleMark } from "./Icon";
import { ThemePicker } from "./ThemePicker";
import { useDialog } from "./useDialog";

type Props = {
  snapshot: DesktopSnapshot; activeTaskId?: string; busy: boolean;
  theme: ResolvedTheme; onTheme: (theme: ThemePreference) => void;
  onCollapse: () => void; onSearch: () => void; onNew: () => void; onOpen: () => void;
  onProject: (project: ProjectSummary) => void; onTask: (task: TaskSummary) => void;
  onSettings: () => void; onAgent: () => void; onExport: () => void; onDiagnostics: () => void;
  onArchive?: (task: TaskSummary, archived: boolean) => Promise<boolean>;
  onDelete?: (task: TaskSummary) => Promise<boolean>;
  operationError?: string;
};

export function Sidebar(props: Props) {
  const { snapshot, activeTaskId, busy } = props;
  const [folded, setFolded] = useState<Record<string, boolean>>({});
  const [showAll, setShowAll] = useState<Record<string, boolean>>({});
  const [showArchived, setShowArchived] = useState(false);
  const [deleting, setDeleting] = useState<TaskSummary>();
  const lifecycleDisabled = busy || snapshot.turns.some(turn => turn.status === "running");
  return <aside className="sidebar" aria-label="项目与任务">
    <div className="sidebar-brand"><SimpleMark /><strong>Simple</strong><span className="brand-tag">本地</span>
      <button className="icon-button" onClick={props.onCollapse} title="收起侧栏（Ctrl/⌘+B）" aria-label="收起侧栏"><Icon name="sidebar" /></button>
    </div>
    <nav className="primary-nav" aria-label="主要操作">
      <button disabled={busy || !snapshot.activeProjectId} onClick={props.onNew}><Icon name="plus" /><span>新任务</span><kbd>Ctrl N</kbd></button>
      <button onClick={props.onSearch}><Icon name="search" /><span>搜索任务</span><kbd>Ctrl K</kbd></button>
      <button disabled={busy} onClick={props.onOpen}><Icon name="folder" /><span>打开项目</span></button>
      <button className="archive-nav" aria-pressed={showArchived} onClick={() => setShowArchived(value => !value)}><Icon name={showArchived ? "chat" : "archive"} /><span>{showArchived ? "返回对话" : "已归档"}</span><span className="archive-count">{snapshot.tasks.filter(task => task.archived).length || ""}</span></button>
    </nav>
    <div className="project-tree">
      <div className="section-heading"><span>项目</span><span>{snapshot.projects.length}</span></div>
      {!snapshot.projects.length ? <p className="sidebar-empty">添加一个本地项目，<br />从这里开始你的工作。</p> : null}
      {snapshot.projects.map((project) => {
        const tasks = findTasks(snapshot.tasks.filter((task) => task.projectId === project.id && Boolean(task.archived) === showArchived), [], "");
        return <section className="project-group" key={project.id}>
          <div className={`project-heading${snapshot.activeProjectId === project.id ? " is-current" : ""}`}>
            <button className="fold-project" aria-label={`${folded[project.id] ? "展开" : "收起"} ${project.name}`} aria-expanded={!folded[project.id]} onClick={() => setFolded((old) => ({ ...old, [project.id]: !old[project.id] }))}><Icon name="chevron" size={12} /></button>
            <button className="project-select" disabled={busy} onClick={() => props.onProject(project)} title={project.path}><Icon name="folder" size={15} /><span>{project.name}</span></button>
            <span className="project-count">{tasks.length}</span>
          </div>
          {!folded[project.id] ? <div className="project-tasks">
            {!tasks.length ? showArchived ? <p className="sidebar-empty">暂无已归档对话</p> : <button className="project-no-task" onClick={() => props.onProject(project)} disabled={busy}>开始第一个任务</button> : null}
            {tasks.slice(0, showAll[project.id] ? undefined : 8).map((task) => <div className="task-list-entry" key={task.id}><button type="button" className={`task-row${task.id === activeTaskId ? " is-active" : ""}`} aria-current={task.id === activeTaskId ? "page" : undefined} disabled={busy || task.archived} title={task.archived ? "恢复后可继续对话" : task.title} onClick={() => props.onTask(task)}>
              <span className={`task-state status-${task.status}`} aria-label={task.status === "running" ? "进行中" : task.status === "failed" ? "需要处理" : ""}>{task.status === "running" ? <span className="mini-spinner" /> : task.status === "failed" ? "!" : null}</span>
              <span className="task-title">{task.title}</span><time dateTime={task.updatedAt}>{relativeTaskTime(task.updatedAt)}</time>
            </button><details className="task-actions" onKeyDown={event => { if (event.key === "Escape") { event.currentTarget.open = false; event.currentTarget.querySelector("summary")?.focus(); } }} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) event.currentTarget.open = false; }}><summary aria-label={`管理对话：${task.title}`} title="管理对话"><Icon name="more" size={16} /></summary><div>
              <button disabled={lifecycleDisabled} onClick={() => void props.onArchive?.(task, !task.archived)}><Icon name={task.archived ? "restore" : "archive"} size={16} /><span>{task.archived ? "恢复对话" : "归档对话"}</span></button>
              <button className="task-menu-delete" disabled={lifecycleDisabled} onClick={event => { event.currentTarget.closest("details")?.removeAttribute("open"); setDeleting(task); }}><Icon name="trash" size={16} /><span>删除对话</span></button>
            </div></details></div>)}
            {tasks.length > 8 ? <button className="show-more-tasks" onClick={() => setShowAll((old) => ({ ...old, [project.id]: !old[project.id] }))}>{showAll[project.id] ? "收起" : `显示另外 ${tasks.length - 8} 个任务`}</button> : null}
          </div> : null}
        </section>;
      })}
    </div>
    <div className="sidebar-bottom">
      <div className="local-runtime"><span className={`connection-dot${snapshot.runtime.core === "connected" ? " connected" : ""}`} /><span>{snapshot.runtime.core === "mock" ? "界面预览 · 模拟数据" : snapshot.runtime.core === "connected" ? "本地工作区" : "内核不可用"}</span></div>
      <div className="sidebar-bottom-actions">
        <button className="settings-link" onClick={props.onSettings}><Icon name="settings" size={17} /><span>设置</span></button>
        <ThemePicker resolvedTheme={props.theme} onChange={props.onTheme} />
        <details className="sidebar-menu"><summary aria-label="更多操作" title="更多操作"><Icon name="more" /></summary><div>
          {activeTaskId ? <button disabled={busy || Boolean(snapshot.activeTurnId)} onClick={props.onAgent}>记忆与 Agent 能力</button> : null}
          {activeTaskId ? <button disabled={busy} onClick={props.onExport}>导出对话</button> : null}
          <button disabled={busy} onClick={props.onDiagnostics}>导出诊断报告</button>
        </div></details>
      </div>
    </div>
    {deleting ? <DeleteConversation task={deleting} error={props.operationError} busy={busy} onClose={() => { if (!busy) setDeleting(undefined); }} onConfirm={async () => { if (await props.onDelete?.(deleting)) setDeleting(undefined); }} /> : null}
  </aside>;
}

function DeleteConversation({ task, error, busy, onClose, onConfirm }: { task: TaskSummary; error?: string; busy: boolean; onClose: () => void; onConfirm: () => Promise<void> }) {
  const ref = useDialog(onClose);
  return <div className="modal-backdrop"><section className="delete-conversation-dialog" ref={ref} role="dialog" aria-modal="true" aria-label="删除对话" tabIndex={-1}>
    <div className="delete-dialog-icon"><Icon name="trash" size={22} /></div>
    <h2>删除这段对话？</h2><p className="delete-dialog-title">{task.title}</p><p className="delete-dialog-description">对话及关联子对话将永久删除。项目文件会保留。</p>
    {error ? <p role="alert">{error}</p> : null}
    <div className="delete-dialog-actions"><button className="delete-cancel" disabled={busy} onClick={onClose}>取消</button><button className="delete-confirm" disabled={busy} onClick={() => void onConfirm()}>{busy ? <><span className="mini-spinner" />正在删除…</> : "确认删除"}</button></div>
  </section></div>;
}
