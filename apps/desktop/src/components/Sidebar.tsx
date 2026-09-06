import { useState } from "react";
import type { DesktopSnapshot, ProjectSummary, TaskSummary } from "../bridge/types";
import type { ResolvedTheme, ThemePreference } from "../theme";
import { findTasks, relativeTaskTime } from "../app/navigation";
import { Icon, SimpleMark } from "./Icon";
import { ThemePicker } from "./ThemePicker";

type Props = {
  snapshot: DesktopSnapshot; activeTaskId?: string; busy: boolean;
  theme: ResolvedTheme; onTheme: (theme: ThemePreference) => void;
  onCollapse: () => void; onSearch: () => void; onNew: () => void; onOpen: () => void;
  onProject: (project: ProjectSummary) => void; onTask: (task: TaskSummary) => void;
  onSettings: () => void; onAgent: () => void; onExport: () => void; onDiagnostics: () => void;
};

export function Sidebar(props: Props) {
  const { snapshot, activeTaskId, busy } = props;
  const [folded, setFolded] = useState<Record<string, boolean>>({});
  const [showAll, setShowAll] = useState<Record<string, boolean>>({});
  return <aside className="sidebar" aria-label="项目与任务">
    <div className="sidebar-brand"><SimpleMark /><strong>Simple</strong><span className="brand-tag">本地</span>
      <button className="icon-button" onClick={props.onCollapse} title="收起侧栏（Ctrl+B）" aria-label="收起侧栏"><Icon name="sidebar" /></button>
    </div>
    <nav className="primary-nav" aria-label="主要操作">
      <button disabled={busy || !snapshot.activeProjectId} onClick={props.onNew}><Icon name="plus" /><span>新任务</span><kbd>Ctrl N</kbd></button>
      <button onClick={props.onSearch}><Icon name="search" /><span>搜索任务</span><kbd>Ctrl K</kbd></button>
      <button disabled={busy} onClick={props.onOpen}><Icon name="folder" /><span>打开项目</span></button>
    </nav>
    <div className="project-tree">
      <div className="section-heading"><span>项目</span><span>{snapshot.projects.length}</span></div>
      {!snapshot.projects.length ? <p className="sidebar-empty">添加一个本地项目，<br />从这里开始你的工作。</p> : null}
      {snapshot.projects.map((project) => {
        const tasks = findTasks(snapshot.tasks.filter((task) => task.projectId === project.id), [], "");
        return <section className="project-group" key={project.id}>
          <div className={`project-heading${snapshot.activeProjectId === project.id ? " is-current" : ""}`}>
            <button className="fold-project" aria-label={`${folded[project.id] ? "展开" : "收起"} ${project.name}`} aria-expanded={!folded[project.id]} onClick={() => setFolded((old) => ({ ...old, [project.id]: !old[project.id] }))}><Icon name="chevron" size={12} /></button>
            <button className="project-select" disabled={busy} onClick={() => props.onProject(project)} title={project.path}><Icon name="folder" size={15} /><span>{project.name}</span></button>
            <span className="project-count">{tasks.length}</span>
          </div>
          {!folded[project.id] ? <div className="project-tasks">
            {!tasks.length ? <button className="project-no-task" onClick={() => props.onProject(project)} disabled={busy}>开始第一个任务</button> : null}
            {tasks.slice(0, showAll[project.id] ? undefined : 8).map((task) => <button type="button" key={task.id} className={`task-row${task.id === activeTaskId ? " is-active" : ""}`} aria-current={task.id === activeTaskId ? "page" : undefined} disabled={busy} title={task.title} onClick={() => props.onTask(task)}>
              <span className={`task-state status-${task.status}`} aria-label={task.status === "running" ? "进行中" : task.status === "failed" ? "需要处理" : ""}>{task.status === "running" ? <span className="mini-spinner" /> : task.status === "failed" ? "!" : null}</span>
              <span className="task-title">{task.title}</span><time dateTime={task.updatedAt}>{relativeTaskTime(task.updatedAt)}</time>
            </button>)}
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
  </aside>;
}
