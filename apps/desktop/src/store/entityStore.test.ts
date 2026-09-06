import { describe, expect, it } from "vitest";
import type { DesktopSnapshot, TimelineEntry } from "../bridge/types";
import { WorkbenchStore } from "./entityStore";

const item = (revision: number, status: TimelineEntry["status"], detail: string): TimelineEntry => ({
  id: "turn-1:assistant",
  taskId: "task-1",
  turnId: "turn-1",
  kind: "assistant",
  title: "Agent",
  detail,
  createdAt: "2026-01-01T00:00:00.000Z",
  revision,
  status,
});

const snapshot = (entry: TimelineEntry): DesktopSnapshot => ({
  projects: [{ id: "project-1", name: "demo", path: "D:/demo" }],
  tasks: [{ id: "task-1", projectId: "project-1", title: "task", goal: "goal", status: "running", permissionLevel: "approval", updatedAt: entry.createdAt }],
  turns: [{ id: "turn-1", taskId: "task-1", status: "running", phase: "sampling", startedAt: entry.createdAt, finishedAt: null, sequence: entry.revision ?? 0 }],
  timeline: [entry],
  actions: [],
  modelProfiles: [],
  contextUsage: [],
  activeModelProfileId: null,
  activeProjectId: "project-1",
  activeTaskId: "task-1",
  activeTurnId: "turn-1",
  taskRevisions: { "task-1": entry.revision ?? 0 },
  runtime: {
    core: "mock",
    storage: "memory",
    network: "offline",
    dataLocation: "memory",
    workspaceSandboxReady: false,
    workspaceSandboxHealth: { status: "unsupported_platform" },
  },
});

describe("WorkbenchStore", () => {
  it("accepts persisted phase/content even when the stream counter is larger than the journal sequence", () => {
    const store = new WorkbenchStore();
    const live = snapshot({...item(5000,"streaming","intermediate partial"),phase:"final_answer"});
    live.taskRevisions["task-1"] = 70;
    live.turns[0].sequence = 70;
    store.hydrate(live);
    const persisted = snapshot({...item(87,"completed","intermediate canonical"),phase:"commentary"});
    persisted.turns[0] = {...persisted.turns[0],status:"completed",phase:"completed",finishedAt: persisted.turns[0].startedAt};
    persisted.activeTurnId = null;
    store.hydrate(persisted);
    expect(store.getSnapshot()?.timeline[0]).toMatchObject({status:"completed",phase:"commentary",detail:"intermediate canonical",revision:87});
    expect(store.getSnapshot()?.activeTurnId).toBeNull();
    const late = {...persisted,timeline:[{...item(6000,"streaming","late text"),phase:"final_answer" as const}]};
    store.hydrate(late);
    expect(store.getSnapshot()?.timeline[0]).toMatchObject({status:"completed",phase:"commentary",detail:"intermediate canonical"});
  });
  it.each(["completed", "cancelled", "failed"] as const)("does not relock a %s turn when an older snapshot arrives", (status) => {
    const store = new WorkbenchStore();
    const ended = snapshot(item(4, "completed", "final"));
    ended.turns[0] = { ...ended.turns[0], status, phase: status, finishedAt: ended.turns[0].startedAt };
    ended.tasks[0].status = status === "cancelled" ? "ready" : status;
    ended.activeTurnId = null;
    store.hydrate(ended);
    store.hydrate(snapshot(item(2, "streaming", "old")));
    expect(store.getSnapshot()?.turns[0].status).toBe(status);
    expect(store.getSnapshot()?.activeTurnId).toBeNull();
  });
  it("ignores another task's candidate and retains the selected task's live turn", () => {
    const store = new WorkbenchStore();
    const next = snapshot(item(4, "streaming", "text"));
    next.turns.push({...next.turns[0], id:"other-turn", taskId:"other-task"});
    next.activeTurnId = "other-turn";
    store.hydrate(next);
    expect(store.getSnapshot()?.activeTurnId).toBe("turn-1");
  });
  it("keeps a newly accepted unknown turn locked until its row arrives", () => {
    const store = new WorkbenchStore();
    const next = snapshot(item(1, "streaming", ""));
    next.turns = [];
    store.hydrate(next);
    expect(store.getSnapshot()?.activeTurnId).toBe("turn-1");
  });
  it("deduplicates item ids and ignores an out-of-order delta after completion", () => {
    const store = new WorkbenchStore();
    store.hydrate(snapshot(item(2, "completed", "final")));
    store.hydrate(snapshot(item(1, "streaming", "partial")));
    expect(store.getSnapshot()?.timeline).toEqual([item(2, "completed", "final")]);
  });

  it("keeps multiple items in one turn", () => {
    const store = new WorkbenchStore();
    const next = snapshot(item(2, "completed", "answer"));
    next.timeline.unshift({ ...item(1, "completed", "read"), id: "tool-1", kind: "file_read" });
    store.hydrate(next);
    expect(store.getSnapshot()?.timeline.map((entry) => entry.id)).toEqual(["tool-1", "turn-1:assistant"]);
  });

  it("does not delete newer entities when an older task snapshot arrives", () => {
    const store = new WorkbenchStore();
    store.hydrate(snapshot(item(4, "completed", "final")));
    const stale = snapshot(item(2, "streaming", "old"));
    stale.timeline = [];
    stale.taskRevisions["task-1"] = 2;
    store.hydrate(stale);
    expect(store.getSnapshot()?.timeline[0]?.detail).toBe("final");
    expect(store.getSnapshot()?.taskRevisions["task-1"]).toBe(4);
  });
});
