import { describe, expect, it } from "vitest";
import {
  projectBackendSnapshot,
  selectActiveTask,
  type BackendSnapshot,
} from "./tauriBridge";

const backend: BackendSnapshot = {
  projects: [
    { id: "project-a", name: "项目 A", path: "E:\\project-a" },
    { id: "project-b", name: "项目 B", path: "E:\\project-b" },
  ],
  tasks: [
    {
      id: "task-a",
      projectId: "project-a",
      title: "已完成任务",
      goal: "验证后端快照转换",
      status: "completed",
      updatedAtMs: 1_725_000_000_000,
    },
    {
      id: "task-b",
      projectId: "project-b",
      title: "另一个任务",
      goal: "验证选择逻辑",
      status: "ready",
      updatedAtMs: 1_725_000_001_000,
    },
  ],
  dataLocation: "E:\\local-agent\\local-agent.db",
};

it("preserves child outcomes independently of the root native status", () => {
  const childReport = {outcomes:[{threadId:"child",turnId:null,status:"unknown" as const}],rejectedOperations:1};
  const snapshot = projectBackendSnapshot({...backend,turns:[{id:"turn-child-report",taskId:"task-a",status:"completed",phase:"completed",startedAtMs:1,finishedAtMs:2,sequence:3,childReport}]});
  expect(snapshot.turns[0].childReport).toEqual(childReport);
  expect(snapshot.turns[0].status).toBe("completed");
});

describe("Tauri snapshot projection", () => {
  it("retains native message phases in persisted snapshots", () => {
    const snapshot = projectBackendSnapshot({...backend, messages: [
      {id:"c",taskId:"task-a",turnId:"t",role:"assistant",phase:"commentary",content:"interim",createdAtMs:1},
      {id:"f",taskId:"task-a",turnId:"t",role:"assistant",phase:"final_answer",content:"answer",createdAtMs:2},
      {id:"old",taskId:"task-a",turnId:"old-turn",role:"assistant",content:"legacy",createdAtMs:3},
    ]});
    expect(snapshot.timeline.find(e=>e.id === "c")?.phase).toBe("commentary");
    expect(snapshot.timeline.find(e=>e.id === "f")?.phase).toBe("final_answer");
    expect(snapshot.timeline.find(e=>e.id === "old")?.phase).toBeUndefined();
  });
  it("maps timestamps, completed status and persistent runtime", () => {
    const snapshot = projectBackendSnapshot(backend);

    expect(snapshot.tasks[0]).toMatchObject({
      id: "task-a",
      status: "completed",
      updatedAt: new Date(backend.tasks[0].updatedAtMs).toISOString(),
    });
    expect(snapshot.runtime).toEqual({
      core: "connected",
      storage: "sqlite",
      network: "offline",
      dataLocation: backend.dataLocation,
      workspaceSandboxReady: false,
      workspaceSandboxHealth: { status: "unsupported_platform" },
    });
    expect(snapshot.tasks[0].permissionLevel).toBe("approval");
  });

  it("maps persisted task permissions while defaulting legacy tasks to approval", () => {
    const snapshot = projectBackendSnapshot({
      ...backend,
      tasks: [
        { ...backend.tasks[0], permissionLevel: "system_full_access" },
        backend.tasks[1],
      ],
    });
    expect(snapshot.tasks[0].permissionLevel).toBe("system_full_access");
    expect(snapshot.tasks[1].permissionLevel).toBe("approval");
  });

  it("preserves the structured sandbox health instead of inferring it from a boolean", () => {
    const snapshot = projectBackendSnapshot({
      ...backend,
      workspaceSandboxReady: false,
      workspaceSandboxHealth: {
        status: "needs_setup",
        backend: "windows_codex",
        expectedSetupVersion: 1,
      },
    });

    expect(snapshot.runtime.workspaceSandboxHealth).toEqual({
      status: "needs_setup",
      backend: "windows_codex",
      expectedSetupVersion: 1,
    });
    expect(snapshot.runtime.workspaceSandboxReady).toBe(false);
  });

  it("keeps a valid selection and rejects a task from another project", () => {
    const selected = projectBackendSnapshot(backend, undefined, "project-b");
    expect(selected.activeProjectId).toBe("project-b");
    expect(selected.activeTaskId).toBe("task-b");

    expect(selectActiveTask(selected.tasks, "project-a", "task-b")).toBe("task-a");
    const refreshed = projectBackendSnapshot(backend, selected);
    expect(refreshed.activeProjectId).toBe("project-b");
    expect(refreshed.activeTaskId).toBe("task-b");
  });

  it("maps context windows and remains compatible with an older backend", () => {
    const withProfile: BackendSnapshot = {
      ...backend,
      modelProfiles: [
        {
          id: "model-a",
          name: "模型 A",
          baseUrl: "http://127.0.0.1:8000/v1",
          model: "local",
          dialect: "standard",
          maxOutputTokens: 4096,
          contextWindowTokens: 65_536,
          timeoutMs: 120_000,
          isDefault: true,
          hasCredential: false,
        },
        {
          id: "model-old",
          name: "旧模型",
          baseUrl: "http://127.0.0.1:9000/v1",
          model: "legacy",
          dialect: "standard",
          maxOutputTokens: null,
          timeoutMs: 120_000,
          isDefault: false,
          hasCredential: false,
        },
      ],
    };

    const profiles = projectBackendSnapshot(withProfile).modelProfiles;
    expect(profiles[0].contextWindowTokens).toBe(65_536);
    expect(profiles[1].contextWindowTokens).toBeNull();
  });

  it("maps per-conversation context usage", () => {
    const snapshot = projectBackendSnapshot({
      ...backend,
      contextUsage: [
        {
          taskId: "task-a",
          estimatedTokens: 8_200,
          contextWindowTokens: 32_768,
          reservedOutputTokens: 4_096,
          messageCount: 24,
          toolExchangeCount: 6,
        },
      ],
    });
    expect(snapshot.contextUsage[0]).toEqual({
      taskId: "task-a",
      estimatedTokens: 8_200,
      contextWindowTokens: 32_768,
      reservedOutputTokens: 4_096,
      messageCount: 24,
      toolExchangeCount: 6,
    });
  });

  it("projects durable revisions, turns and structured tool items", () => {
    const snapshot = projectBackendSnapshot({
      ...backend,
      tasks: [{ ...backend.tasks[0], status: "running", lastSequence: 9 }],
      turns: [{
        id: "turn-a",
        taskId: "task-a",
        status: "running",
        phase: "executing_tools",
        startedAtMs: 1_725_000_000_100,
        finishedAtMs: null,
        sequence: 9,
      }],
      messages: [{
        id: "message-a",
        taskId: "task-a",
        turnId: "turn-a",
        role: "assistant",
        content: "完成",
        createdAtMs: 1_725_000_000_300,
      }],
      toolItems: [{
        id: "tool-a",
        taskId: "task-a",
        turnId: "turn-a",
        kind: "file_read",
        title: "读取文件",
        detail: "README.md",
        createdAtMs: 1_725_000_000_200,
      }],
    });
    expect(snapshot.taskRevisions).toEqual({ "task-a": 9 });
    expect(snapshot.activeTurnId).toBe("turn-a");
    expect(snapshot.turns[0]).toMatchObject({ id: "turn-a", phase: "executing_tools", sequence: 9 });
    expect(snapshot.timeline.map((item) => item.id)).toEqual(["tool-a", "message-a"]);
    expect(snapshot.timeline[0]).toMatchObject({ kind: "file_read", lowValue: true });
  });

  it("shows preparation items only while their action is running", () => {
    const action = {
      id: "action-a",
      taskId: "task-a",
      turnId: "turn-a",
      kind: "run_command" as const,
      title: "运行终端命令",
      detail: "npm test",
      diff: null,
      result: null,
      canUndo: false,
      createdAtMs: 1_725_000_000_200,
    };
    const toolItem = {
      id: "tool-command-a",
      taskId: "task-a",
      turnId: "turn-a",
      kind: "command" as const,
      title: "准备命令",
      detail: "npm test",
      createdAtMs: 1_725_000_000_100,
    };

    const running = projectBackendSnapshot({
      ...backend,
      messages: [],
      toolItems: [toolItem],
      actions: [{ ...action, status: "running" }],
    });
    expect(running.timeline).toContainEqual(expect.objectContaining({
      id: "tool-command-a",
      status: "running",
    }));

    for (const status of ["applied", "failed", "rejected"] as const) {
      const terminal = projectBackendSnapshot({
        ...backend,
        messages: [],
        toolItems: [toolItem],
        actions: [{ ...action, status }],
      });
      expect(terminal.timeline.map((item) => item.id)).not.toContain("tool-command-a");
    }
  });
});
