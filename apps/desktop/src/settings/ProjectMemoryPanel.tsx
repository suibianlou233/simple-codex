import { useEffect, useState } from "react";
import type { DesktopBridge, ProjectMemoryView } from "../bridge/types";
import { ProjectMemoryNotesEditor } from "./ProjectMemoryNotesEditor";

const labels = {
  missing: "尚未生成此项记忆",
  tooLarge: "内容超过 256 KB，暂不在此处展开；记忆文件未被修改",
  unreadable: "无法读取此项记忆，请检查文件或稍后刷新",
  unsafe: "此项是链接或非普通文件，已阻止读取",
};

export function MemoryDocuments({ view }: { view: ProjectMemoryView }) {
  return <div className="project-memory-documents">
    <p className="project-memory-source">所属项目：{view.projectPath}</p>
    <p>{view.legacyHistory ? "这是旧会话使用的独立记忆区，未与新会话的记忆自动合并。" : "这是当前会话使用的项目记忆区。"}</p>
    <small>以下为主要摘要、索引和提炼记录；索引引用的其他文件尚未在此展开。只读展示，不会改写记忆文件。</small>
    {view.documents.length === 0 ? <p>当前环境没有可展示的记忆文件。</p> : view.documents.map((document) => <details key={document.name}>
      <summary>{document.title}<span>{document.status === "ready" ? "查看" : "查看状态"}</span></summary>
      {document.status === "ready" ? <pre>{document.content || "文件为空"}</pre> : <p role={document.status === "missing" ? undefined : "status"}>{labels[document.status] ?? "未知读取状态，请刷新"}</p>}
    </details>)}
  </div>;
}

export function ProjectMemoryPanel({ bridge, taskId, revision = 0 }: { bridge: DesktopBridge; taskId: string; revision?: number }) {
  const [refresh, setRefresh] = useState(0);
  const [state, setState] = useState<{ taskId: string; view?: ProjectMemoryView; failed?: boolean }>();
  useEffect(() => {
    let active = true;
    setState({ taskId });
    void bridge.loadProjectMemory(taskId).then((view) => {
      if (active) setState(view.taskId === taskId ? { taskId, view } : { taskId, failed: true });
    }).catch(() => { if (active) setState({ taskId, failed: true }); });
    return () => { active = false; };
  }, [bridge, taskId, refresh, revision]);
  const current = state?.taskId === taskId ? state : undefined;
  return <section className="project-memory-panel" aria-label="项目记忆内容">
    <div className="project-memory-heading"><strong>记忆内容</strong><button type="button" onClick={() => setRefresh((value) => value + 1)}>刷新</button></div>
    <ProjectMemoryNotesEditor key={`notes:${taskId}`} bridge={bridge} taskId={taskId} />
    {current?.view ? <MemoryDocuments key={taskId} view={current.view} /> : current?.failed ? <p role="alert">未能读取项目记忆，请稍后刷新。这不表示记忆为空。</p> : <p role="status">正在读取项目记忆…</p>}
  </section>;
}
