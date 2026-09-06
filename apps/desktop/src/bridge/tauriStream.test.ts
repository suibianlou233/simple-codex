import { afterEach, expect, it, vi } from "vitest";
import type { DesktopSnapshot } from "./types";
import type { BackendSnapshot } from "./tauriBridge";

const wire = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  snapshot: {} as BackendSnapshot,
  loadSnapshot: undefined as undefined | (() => Promise<BackendSnapshot>),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, listener: (event: { payload: unknown }) => void) => {
    wire.listeners.set(name, listener);
    return () => wire.listeners.delete(name);
  },
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (name: string) => name === "load_snapshot" ? wire.loadSnapshot ? wire.loadSnapshot() : wire.snapshot : undefined,
}));
import { TauriDesktopBridge } from "./tauriBridge";
import { WorkbenchStore } from "../store/entityStore";
import { TaskTimeline } from "../items/TaskTimeline";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

afterEach(() => { vi.useRealTimers(); wire.listeners.clear(); wire.loadSnapshot = undefined; });

it("folds all persisted commentary through the real bridge/store/render chain after high-counter streaming", async () => {
  vi.useFakeTimers();
  wire.snapshot = {projects:[{id:"p",name:"fixture",path:"E:/fixture"}],
    tasks:[{id:"t",projectId:"p",title:"fixture",goal:"fixture",status:"running",updatedAtMs:1,lastSequence:67}],
    turns:[{id:"turn",taskId:"t",status:"running",phase:"sampling",startedAtMs:1,finishedAtMs:null,sequence:67}],
    messages:[],dataLocation:"fixture.db"};
  const bridge = new TauriDesktopBridge();
  const store = new WorkbenchStore();
  store.hydrate(await bridge.load());
  const unsubscribe = bridge.subscribe(store.hydrate);
  for (const [index, id] of ["note-1", "note-2", "note-3", "answer"].entries()) {
    wire.listeners.get("turn-stream")?.({payload:{eventId:id,sequence:index === 0 ? 5 : 5000+index,
      taskId:"t",turnId:"turn",itemId:id,kind:"delta",phase:index === 0 ? "commentary" : "final_answer",content:id}});
  }
  await vi.advanceTimersByTimeAsync(33);
  Object.assign(wire.snapshot.tasks[0],{status:"completed",lastSequence:87});
  Object.assign(wire.snapshot.turns![0],{status:"completed",phase:"completed",finishedAtMs:4,sequence:87});
  wire.snapshot.messages = ["note-1", "note-2", "note-3", "answer"].map((id,index) => ({
    id,taskId:"t",turnId:"turn",role:"assistant",phase:id === "answer" ? "final_answer" : "commentary",content:id,createdAtMs:index+1,
  }));
  wire.listeners.get("turn-stream")?.({payload:{eventId:"done",sequence:6000,taskId:"t",turnId:"turn",kind:"finished"}});
  await vi.runAllTimersAsync();
  const snapshot = store.getSnapshot()!;
  expect(snapshot.timeline.filter((item) => item.phase === "commentary")).toHaveLength(3);
  const html = renderToStaticMarkup(createElement(TaskTimeline, {
    entries:snapshot.timeline,turns:snapshot.turns,actions:[],activeTurnId:snapshot.activeTurnId,disabled:false,
    onApprove:async()=>{},onReject:async()=>{},onUndo:async()=>{},onCancel:async()=>true,
    onRevise:async()=>true,onRegenerate:async()=>{},onBranch:async()=>{},
  }));
  expect(html).toContain("执行过程 · 3 条说明");
  expect(html.match(/class="completed-commentary"/g)).toHaveLength(1);
  expect(html.indexOf("note-3")).toBeLessThan(html.indexOf("</details>"));
  expect(html.indexOf("answer")).toBeGreaterThan(html.indexOf("</details>"));
  unsubscribe();
});

it.each([false, true])("does not let background events replace the selected task's active turn (selected running=%s)", async (selectedRunning) => {
  vi.useFakeTimers();
  wire.snapshot = {
    projects:[{id:"p",name:"fixture",path:"E:/fixture"}],
    tasks:[{id:"a",projectId:"p",title:"A",goal:"A",status:"running",updatedAtMs:1},
      {id:"b",projectId:"p",title:"B",goal:"B",status:selectedRunning ? "running" : "completed",updatedAtMs:1}],
    turns:[{id:"turn-a",taskId:"a",status:"running",phase:"sampling",startedAtMs:1,finishedAtMs:null,sequence:1},
      {id:"turn-b",taskId:"b",status:selectedRunning ? "running" : "completed",phase:selectedRunning ? "sampling" : "completed",startedAtMs:1,finishedAtMs:selectedRunning ? null : 2,sequence:1}],
    messages:[], dataLocation:"fixture.db",
  };
  const bridge = new TauriDesktopBridge();
  let latest = await bridge.load();
  const unsubscribe = bridge.subscribe((next) => { latest = next; });
  await bridge.selectTask("b");
  const expected = selectedRunning ? "turn-b" : null;
  for (const [index, kind] of ["started", "delta", "reset", "delta"].entries()) {
    wire.listeners.get("turn-stream")?.({payload:{eventId:`${kind}-${index}`,sequence:2,taskId:"a",turnId:"turn-a",itemId:"a-output",kind,content:"background",phase:"final_answer"}});
    await vi.advanceTimersByTimeAsync(33);
    expect(latest.activeTaskId).toBe("b");
    expect(latest.activeTurnId).toBe(expected);
  }
  expect(latest.timeline.find((entry) => entry.id === "a-output")?.taskId).toBe("a");
  await bridge.load();
  expect(latest.activeTurnId).toBe(expected);
  await bridge.selectTask("a");
  expect(latest.activeTurnId).toBe("turn-a");
  unsubscribe();
});

it("rejects a late approval refresh after the turn has completed", async () => {
  wire.snapshot = {projects:[{id:"p",name:"fixture",path:"E:/fixture"}],
    tasks:[{id:"t",projectId:"p",title:"fixture",goal:"fixture",status:"running",updatedAtMs:1,lastSequence:1}],
    turns:[{id:"turn",taskId:"t",status:"running",phase:"waiting_approval",startedAtMs:1,finishedAtMs:null,sequence:1}],
    messages:[],dataLocation:"fixture.db"};
  const bridge = new TauriDesktopBridge();
  let latest = await bridge.load();
  const unsubscribe = bridge.subscribe((next) => { latest = next; });
  const old = structuredClone(wire.snapshot);
  let release!: (snapshot: BackendSnapshot) => void;
  wire.loadSnapshot = () => new Promise((resolve) => { release = resolve; });
  wire.listeners.get("turn-stream")?.({payload:{eventId:"approval",sequence:1,taskId:"t",turnId:"turn",kind:"approval_required"}});
  wire.loadSnapshot = undefined;
  Object.assign(wire.snapshot.turns![0],{status:"completed",phase:"completed",finishedAtMs:3,sequence:3});
  Object.assign(wire.snapshot.tasks[0],{status:"completed",lastSequence:3});
  await bridge.load();
  release(old);
  await new Promise((resolve) => setTimeout(resolve,0));
  expect(latest.turns[0].status).toBe("completed");
  expect(latest.activeTurnId).toBeNull();
  unsubscribe();
});

it.each(["completion_pending", "submission_pending", "delegations_updated"])("keeps %s active on refresh failure and ignores a stale pending snapshot after completion", async (pendingKind) => {
  wire.snapshot = {
    projects: [{id:"p",name:"fixture",path:"E:/fixture"}],
    tasks: [{id:"t",projectId:"p",title:"fixture",goal:"fixture",status:"running",updatedAtMs:1}],
    turns: [{id:"turn",taskId:"t",status:"running",phase:"sampling",startedAtMs:1,finishedAtMs:null,sequence:1}],
    messages: [], dataLocation:"fixture.db",
  };
  const bridge = new TauriDesktopBridge();
  let latest = await bridge.load();
  const unsubscribe = bridge.subscribe(snapshot => { latest = snapshot; });
  const emit = (eventId: string, kind: string) => wire.listeners.get("turn-stream")?.({payload:{eventId,sequence:2,taskId:"t",turnId:"turn",kind}});
  const pending = structuredClone(wire.snapshot);
  const phase = pendingKind === "submission_pending" ? "checking_submission" : "waiting_children";
  pending.turns![0].phase = phase;
  if (pendingKind === "delegations_updated") pending.turns![0].childReport = {outcomes:[],rejectedOperations:0,assignments:[{
    senderThreadId:"native-root",senderTurnId:"native-turn",itemId:"dispatch",receivers:["child"],instruction:"检查任务搜索",followUp:false,status:"dispatched",
  }]};
  wire.snapshot = pending;
  emit("pending", pendingKind);
  await vi.waitFor(() => expect(latest.turns[0].phase).toBe(phase));
  expect(latest.activeTurnId).toBe("turn");
  if (pendingKind === "delegations_updated") expect(latest.turns[0].childReport?.assignments?.[0].instruction).toBe("检查任务搜索");

  wire.loadSnapshot = async () => { throw new Error("fixture status unavailable"); };
  emit("unavailable", pendingKind);
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(latest.activeTurnId).toBe("turn");
  expect(latest.turns[0].status).toBe("running");

  let release: (snapshot: BackendSnapshot) => void = () => { throw new Error("uninitialized response"); };
  wire.loadSnapshot = () => new Promise(resolve => { release = resolve; });
  emit("late-pending", pendingKind);
  wire.loadSnapshot = undefined;
  wire.snapshot = structuredClone(pending);
  Object.assign(wire.snapshot.turns![0], {status:"completed",phase:"completed",finishedAtMs:3,sequence:3});
  wire.snapshot.tasks[0].status = "completed";
  emit("finished", "finished");
  await vi.waitFor(() => expect(latest.turns[0].status).toBe("completed"));
  release(pending);
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(latest.turns[0].status).toBe("completed");
  expect(latest.activeTurnId).toBeNull();
  unsubscribe();
});

it("preserves phase through batching, deduplicates wire events and rejects late text after hydration", async () => {
  vi.useFakeTimers();
  wire.snapshot = {
    projects: [{id:"p",name:"fixture",path:"E:/fixture"}],
    tasks: [{id:"t",projectId:"p",title:"fixture",goal:"fixture",status:"running",updatedAtMs:1}],
    turns: [{id:"turn",taskId:"t",status:"running",phase:"sampling",startedAtMs:1,finishedAtMs:null,sequence:1}],
    messages: [], dataLocation:"fixture.db",
  };
  const bridge = new TauriDesktopBridge();
  let latest: DesktopSnapshot = await bridge.load();
  const unsubscribe = bridge.subscribe(snapshot => { latest = snapshot; });
  const emit = (eventId: string, sequence: number, phase: "final_answer" | "commentary", content: string, itemId = "answer") =>
    wire.listeners.get("turn-stream")?.({payload:{eventId,sequence,phase,content,itemId,taskId:"t",turnId:"turn",kind:"delta"}});
  emit("d1", 1, "final_answer", "完成");
  emit("d1", 1, "final_answer", "完成");
  emit("d2", 2, "final_answer", "修改");
  emit("d3", 3, "commentary", "内部操作", "note");
  await vi.advanceTimersByTimeAsync(33);
  expect(latest.timeline.find(item => item.id === "answer")).toMatchObject({phase:"final_answer",detail:"完成修改",status:"streaming"});
  expect(latest.timeline.find(item => item.id === "note")?.phase).toBe("commentary");
  wire.snapshot.turns = [{id:"turn",taskId:"t",status:"completed",phase:"completed",startedAtMs:1,finishedAtMs:2,sequence:4}];
  wire.snapshot.tasks[0].status = "completed";
  wire.snapshot.messages = [{id:"answer",taskId:"t",turnId:"turn",phase:"final_answer",role:"assistant",content:"完成修改",createdAtMs:2}];
  wire.listeners.get("turn-stream")?.({payload:{eventId:"done",sequence:4,taskId:"t",turnId:"turn",kind:"finished"}});
  await vi.runAllTimersAsync();
  emit("late", 5, "final_answer", "不应追加");
  emit("late-new-item", 6, "commentary", "不应重新打开轮次", "unknown-late-item");
  wire.listeners.get("turn-stream")?.({payload:{eventId:"late-start",sequence:7,taskId:"t",turnId:"turn",kind:"started"}});
  wire.listeners.get("turn-stream")?.({payload:{eventId:"late-reset",sequence:8,taskId:"t",turnId:"turn",kind:"reset",itemId:"answer"}});
  await vi.advanceTimersByTimeAsync(33);
  expect(latest.timeline.filter(item => item.id === "answer")).toHaveLength(1);
  expect(latest.timeline.find(item => item.id === "answer")).toMatchObject({phase:"final_answer",detail:"完成修改",status:"completed"});
  expect(latest.timeline.some(item => item.id === "unknown-late-item")).toBe(false);
  expect(latest.activeTurnId).toBeNull();
  unsubscribe();
});
