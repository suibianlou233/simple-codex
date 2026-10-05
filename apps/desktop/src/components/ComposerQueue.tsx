import {createContext, useContext, useEffect, useRef, useState, type PropsWithChildren} from "react";
import type {DesktopBridge, TurnSummary} from "../bridge/types";
import "./composerQueue.css";

export type QueuedInstruction = {id: string; text: string};
export type QueueJournal = {items: QueuedInstruction[]; waitFor?: string; inflight?: string; awaiting?: boolean; paused?: boolean; afterSequence?: number; restoreError?: string; notes: string[]};
export function mayDispatch(q: QueueJournal, turns: TurnSummary[]) {
  return Boolean(q.items.length && !q.inflight && !q.awaiting && !q.paused && !turns.some(t => t.status === "running") && turns.some(t => t.id === q.waitFor && t.status === "completed"));
}
const pending = new Set<string>();
const blockedPhases = new Set(["preparing_kernel", "preparing", "checking_submission", "submission_recovery_required", "checking_completion"]);
type QueueContext = {journal: QueueJournal; error: string; busy: boolean; canQueue: boolean; canSteer: boolean; active: boolean; enqueue: (text: string) => boolean; steer: (text: string) => Promise<boolean>; remove: (id: string) => void; resume: () => void};
const Context = createContext<QueueContext | null>(null);
export const useComposerQueue = () => useContext(Context);

export function ComposerQueueProvider({taskId, bridge, turns, onSteer, children}: PropsWithChildren<{taskId?: string; bridge: DesktopBridge; turns: TurnSummary[]; onSteer?: (turnId: string, content: string) => Promise<unknown>}>) {
  const storageKey = taskId ? `simple-code-queue:${taskId}` : "";
  const [journal, setJournal] = useState<QueueJournal>(() => {
    if (!storageKey) return {items: [], notes: []};
    try {
      const raw = localStorage.getItem(storageKey);
      if (!raw) return {items: [], notes: []};
      const q = JSON.parse(raw) as QueueJournal;
      if (!Array.isArray(q.items) || !q.items.every(i => i && typeof i.id === "string" && typeof i.text === "string")) throw new Error();
      return {...q, notes: Array.isArray(q.notes) ? q.notes : [], paused: Boolean(q.items.length || q.inflight || q.awaiting)};
    } catch {return {items: [], notes: [], paused: true, restoreError: "队列恢复失败，请核对之前的发送记录。"};}
  });
  const [error, setError] = useState(journal.restoreError ?? ""), [busy, setBusy] = useState(false);
  const current = useRef(journal), mounted = useRef(true); current.current = journal;
  const active = turns.find(t => t.status === "running");
  useEffect(() => {mounted.current = true; return () => {mounted.current = false;};}, []);
  function commit(next: QueueJournal) {
    if (!storageKey) throw new Error("没有可用的任务");
    localStorage.setItem(storageKey, JSON.stringify(next));
    current.current = next;
    if (mounted.current) setJournal(next);
  }
  async function sendNext() {
    const q = current.current;
    if (!taskId || pending.has(storageKey) || active || q.inflight || !q.items.length) return;
    const item = q.items[0]; pending.add(storageKey); setBusy(true); setError("");
    try {
      commit({...q, inflight: item.id, paused: false, afterSequence: Math.max(0, ...turns.map(t => t.sequence))});
      await bridge.sendMessage(taskId, item.text);
      if (!mounted.current) return;
      const next = current.current;
      commit({...next, items: next.items.filter(i => i.id !== item.id), inflight: undefined, waitFor: undefined, awaiting: true});
    } catch (e) {
      if (mounted.current) setError(`发送结果需确认：${String(e)}。不会自动重发，请核对对话后处理。`);
      try {commit({...current.current, paused: true});} catch { /* Keep the durable inflight marker. */ }
    } finally {pending.delete(storageKey); if (mounted.current) setBusy(false);}
  }
  useEffect(() => {
    if (!taskId) return;
    try {
      const q = current.current;
      if (q.paused || q.inflight) return;
      if (q.awaiting) {
        const next = turns.filter(t => t.sequence > (q.afterSequence ?? 0)).sort((a,b) => a.sequence-b.sequence)[0];
        if (next) commit({...q, awaiting: false, waitFor: next.id});
        return;
      }
      if (q.items.length && turns.some(t => t.id === q.waitFor && (t.status === "failed" || t.status === "cancelled"))) {commit({...q, paused: true}); return;}
      if (mayDispatch(q, turns)) void sendNext();
    } catch (e) {setError(String(e));}
  }, [turns, journal, taskId]);
  const canQueue = Boolean(taskId && active && !blockedPhases.has(active.phase));
  function enqueue(text: string) {
    if (!canQueue || !text.trim()) return false;
    try {
      const q = current.current;
      commit({...q, items: [...q.items, {id: crypto.randomUUID(), text}], waitFor: q.items.length || q.awaiting ? q.waitFor : active!.id});
      setError(""); return true;
    } catch (e) {setError(`队列未保存，草稿已保留：${String(e)}`); return false;}
  }
  async function steer(text: string) {
    if (!canQueue || !active || !onSteer || !text.trim() || busy) return false;
    setBusy(true); setError("");
    try {await onSteer(active.id, text); return true;}
    catch (e) {setError(`追加结果需确认，草稿已保留：${String(e)}`); return false;}
    finally {if (mounted.current) setBusy(false);}
  }
  function remove(id: string) {
    if (busy) return;
    try {commit({...current.current, items: current.current.items.filter(i => i.id !== id), inflight: current.current.inflight === id ? undefined : current.current.inflight}); setError("");}
    catch (e) {setError(String(e));}
  }
  function resume() {
    if (busy || journal.inflight) return;
    if (!active) {void sendNext(); return;}
    try {commit({...current.current, paused: false, awaiting: false, waitFor: active.id});} catch (e) {setError(String(e));}
  }
  return <Context.Provider value={{journal, error, busy, canQueue, canSteer: canQueue && Boolean(onSteer), active: Boolean(active), enqueue, steer, remove, resume}}>{children}</Context.Provider>;
}

export function ComposerQueueTray() {
  const queue = useComposerQueue();
  if (!queue || (!queue.journal.items.length && !queue.error)) return null;
  return <section className="composer-queue" aria-label="待发送任务">
    {queue.journal.items.length ? <><header><span>{queue.journal.paused || queue.journal.inflight ? "队列已暂停" : "接下来"}</span><small>{queue.journal.items.length} 项任务</small></header>
      <ol>{queue.journal.items.map(item => <li key={item.id}><span title={item.text}>{item.text}</span><button type="button" disabled={queue.busy} aria-label={queue.journal.inflight === item.id ? "已核对，移除待确认项" : "移除排队任务"} onClick={() => queue.remove(item.id)}>×</button></li>)}</ol>
      {queue.journal.paused && !queue.journal.inflight ? <button type="button" className="queue-resume" disabled={queue.busy} onClick={queue.resume}>{queue.active ? "继续等待当前轮" : "现在发送下一条"}</button> : null}
      {queue.journal.inflight ? <small>发送结果待确认，请核对对话后移除，避免重复发送。</small> : null}</> : null}
    {queue.error ? <p role="alert">{queue.error}</p> : null}
  </section>;
}
