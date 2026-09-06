import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { DesktopSnapshot, TaskSummary } from "../bridge/types";
import { findTasks } from "../app/navigation";
import { isComposingKey } from "../app/interactions";
import { Icon } from "./Icon";
import { useDialog } from "./useDialog";

export function TaskSearch({ snapshot, onClose, onSelect }: { snapshot: DesktopSnapshot; onClose: () => void; onSelect: (task: TaskSummary) => void }) {
  const [query, setQuery] = useState("");
  const ref = useDialog(onClose);
  const composingRef = useRef(false);
  const tasks = useMemo(() => findTasks(snapshot.tasks, snapshot.timeline, query, snapshot.projects), [query, snapshot]);
  const handleInputKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    // Let the input method finish its candidate instead of opening a task or moving focus.
    if (isComposingKey({ isComposing: composingRef.current || event.nativeEvent.isComposing, keyCode: event.nativeEvent.keyCode })) return;
    if (event.key === "Enter" && tasks[0]) onSelect(tasks[0]);
    if (event.key === "ArrowDown") { event.preventDefault(); ref.current?.querySelector<HTMLButtonElement>(".search-result")?.focus(); }
  };
  return <div className="modal-backdrop search-backdrop" onMouseDown={onClose}>
    <section className="task-search-dialog" ref={ref} role="dialog" aria-modal="true" aria-label="搜索任务" tabIndex={-1} onMouseDown={(event) => event.stopPropagation()}>
      <div className="task-search-input"><Icon name="search" /><input aria-label="搜索本地对话" placeholder="搜索所有项目的任务与对话…" value={query} onChange={(event) => setQuery(event.target.value)} onCompositionStart={() => { composingRef.current = true; }} onCompositionEnd={() => { composingRef.current = false; }} onKeyDown={handleInputKeyDown} /><button className="icon-button" aria-label="关闭搜索" onClick={onClose}><Icon name="close" size={16} /></button></div>
      <p className="search-caption">{query ? `找到 ${tasks.length} 个任务` : "最近的任务"}<span>项目名 · 任务 · 已加载对话</span></p>
      <div className="search-results" onKeyDown={(event) => { if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return; const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>(".search-result")); const current = buttons.indexOf(document.activeElement as HTMLButtonElement); const next = current + (event.key === "ArrowDown" ? 1 : -1); event.preventDefault(); buttons[(next + buttons.length) % buttons.length]?.focus(); }}>
        {tasks.map((task) => <button className="search-result" key={task.id} onClick={() => onSelect(task)}><Icon name="chat" /><span><strong>{task.title}</strong><small>{snapshot.projects.find((p) => p.id === task.projectId)?.name}</small></span><Icon name="chevron" size={14} /></button>)}
        {!tasks.length ? <p className="search-empty">{query ? "没有找到匹配的任务，试试其他关键词。" : "还没有任务。打开项目后，发出第一条消息。"}</p> : null}
      </div>
      <footer><span>↑ ↓ 选择 · Enter 打开</span><span>Esc 关闭</span></footer>
    </section>
  </div>;
}
