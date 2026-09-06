import { useEffect, useRef, useState } from "react";
import type { DesktopBridge, ProjectMemoryNotes } from "../bridge/types";

// Parent keys by task: a late read/save must never populate another project.
export function ProjectMemoryNotesEditor({bridge, taskId}: {bridge:DesktopBridge; taskId:string}) {
  const [notes, setNotes] = useState<ProjectMemoryNotes>();
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [failed, setFailed] = useState(false);
  const [confirmation, setConfirmation] = useState<"delete" | "reload">();
  const [refresh, setRefresh] = useState(0);
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  useEffect(() => {
    let active = true;
    setNotes(undefined); setFailed(false); setNotice("");
    void bridge.loadProjectMemoryNotes(taskId).then((value) => {
      if (!active) return;
      if (value.taskId !== taskId || !Number.isSafeInteger(value.revision) || value.revision < 0) throw new Error("invalid memory response");
      setNotes(value); setDraft(value.content);
    }).catch(() => { if (active) { setFailed(true); setNotice("未能读取确认记忆。这不表示记忆为空，请重试。"); } });
    return () => { active = false; };
  }, [bridge, taskId, refresh]);
  const dirty = notes !== undefined && notes.content !== draft;
  const oversized = new TextEncoder().encode(draft).length > 8192;
  const save = async (content: string) => {
    if (!notes || busy) return;
    setBusy(true); setConfirmation(undefined); setFailed(false); setNotice("");
    try {
      const result = await bridge.saveProjectMemoryNotes(taskId, content, notes.revision);
      if (!alive.current) return;
      if (result.taskId !== taskId || result.revision !== notes.revision + 1 || result.content !== content) throw new Error("unconfirmed memory save");
      setNotes(result); setDraft(result.content); setNotice(content ? "已保存，将在启用记忆的下一轮请求中使用。" : "已删除确认记忆；旧对话和自动摘要未被删除。");
    } catch (error) {
      if (alive.current) { setFailed(true); setNotice(String(error).includes("确认记忆不能保存密钥") ? "未保存：请移除密钥或认证信息后重试。" : "未确认保存成功，编辑内容已保留。请确认项目没有运行中的任务；如有其他窗口编辑，请加载最新版本后重试。"); }
    } finally { if (alive.current) setBusy(false); }
  };
  return <section className="project-memory-notes" aria-label="用户确认的项目记忆">
    <details>
      <summary>用户确认的项目记忆</summary>
      <p>记录已核实的项目约定或纠正内容，与自动摘要分开保存，不会被自动整理覆盖。同项目任务共享，其他项目不共享。</p>
      <p>仅在任务启用记忆时随下一轮请求提供给模型。不要填写密钥。旧对话可能仍包含旧内容，模型是否正确采用仍需核实。</p>
      <label>确认记忆内容<textarea aria-label="确认记忆内容" rows={5} value={draft} disabled={!notes || busy} onChange={(event) => {setDraft(event.target.value); setNotice("");}} /></label>
      <small>最多 8 KB。切换任务前请保存编辑；删除不会清除历史对话或自动摘要。</small>
      {oversized ? <p role="alert">内容超过 8 KB，请精简后保存。</p> : null}
      <div className="action-controls">
        <button type="button" disabled={!dirty || busy || oversized} onClick={() => void save(draft)}>保存确认记忆</button>
        <button type="button" disabled={busy} onClick={() => dirty ? setConfirmation("reload") : setRefresh((value) => value+1)}>加载最新版本</button>
        <button type="button" disabled={!notes?.content || busy} onClick={() => setConfirmation("delete")}>删除确认记忆</button>
      </div>
      {confirmation ? <div role="group" aria-label="确认记忆操作">
        <p>{confirmation === "delete" ? "删除这份用户确认的项目记忆？旧对话和自动摘要不会删除，其他未保存的编辑也将放弃。" : "加载最新版本会放弃当前未保存的编辑，是否继续？"}</p>
        <button type="button" onClick={() => setConfirmation(undefined)}>取消</button>
        <button type="button" onClick={() => { if (confirmation === "delete") void save(""); else {setConfirmation(undefined); setRefresh((value) => value+1);} }}>确认{confirmation === "delete" ? "删除" : "加载"}</button>
      </div> : null}
      {busy ? <p role="status">正在保存…</p> : notice ? <p role={failed ? "alert" : "status"}>{notice}</p> : !notes ? <p role="status">正在读取确认记忆…</p> : null}
    </details>
  </section>;
}
