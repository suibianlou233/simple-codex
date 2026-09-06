import { describe, expect, it, vi } from "vitest";
import { MemoryDesktopBridge } from "./memoryBridge";

describe("MemoryDesktopBridge", () => {
  it("shares manual memory within a project and rejects stale edits after deletion", async () => {
    const bridge = new MemoryDesktopBridge();
    await bridge.openProject();
    const projectId = (await bridge.load()).projects[0].id;
    await bridge.createTask({projectId,title:"A",goal:"fixture"});
    const first = (await bridge.load()).activeTaskId!;
    await bridge.createTask({projectId,title:"B",goal:"fixture"});
    const second = (await bridge.load()).activeTaskId!;
    await bridge.saveProjectMemoryNotes(first,"confirmed",0);
    expect(await bridge.loadProjectMemoryNotes(second)).toEqual({taskId:second,content:"confirmed",revision:1});
    await bridge.saveProjectMemoryNotes(second,"",1);
    await expect(bridge.saveProjectMemoryNotes(first,"stale",1)).rejects.toThrow();
    expect(await bridge.loadProjectMemoryNotes(first)).toEqual({taskId:first,content:"",revision:2});
    await expect(bridge.saveProjectMemoryNotes(first,"中".repeat(3000),2)).rejects.toThrow();
  });
  it("starts offline without projects or tasks", async () => {
    const bridge = new MemoryDesktopBridge();
    const snapshot = await bridge.load();

    expect(snapshot.projects).toEqual([]);
    expect(snapshot.tasks).toEqual([]);
    expect(snapshot.runtime.network).toBe("offline");
  });

  it("creates a local project and task timeline", async () => {
    const bridge = new MemoryDesktopBridge();
    await bridge.openProject();
    const opened = await bridge.load();
    const project = opened.projects[0];

    await bridge.createTask({
      projectId: project.id,
      title: "实现桌面骨架",
      goal: "建立一个可以恢复任务的三栏界面",
    });

    const snapshot = await bridge.load();
    expect(snapshot.tasks).toHaveLength(1);
    expect(snapshot.activeTaskId).toBe(snapshot.tasks[0].id);
    expect(snapshot.timeline).toEqual([]);
    expect(snapshot.tasks[0].goal).toContain("三栏界面");
    expect(snapshot.tasks[0].permissionLevel).toBe("approval");
  });

  it("persists an explicit permission level and allows changing an idle task", async () => {
    const bridge = new MemoryDesktopBridge();
    await bridge.openProject();
    const project = (await bridge.load()).projects[0];
    await bridge.createTask({
      projectId: project.id,
      title: "权限测试",
      goal: "验证任务权限",
      permissionLevel: "project_full_access",
    });
    const created = await bridge.load();
    expect(created.tasks[0].permissionLevel).toBe("project_full_access");

    await bridge.setTaskPermission(created.tasks[0].id, "system_full_access");
    expect((await bridge.load()).tasks[0].permissionLevel).toBe("system_full_access");
  });

  it("publishes immutable snapshots after changes", async () => {
    const bridge = new MemoryDesktopBridge();
    const listener = vi.fn();
    const unsubscribe = bridge.subscribe(listener);

    await bridge.openProject();
    expect(listener).toHaveBeenCalledTimes(1);

    const published = listener.mock.calls[0][0];
    published.projects.length = 0;
    expect((await bridge.load()).projects).toHaveLength(1);

    unsubscribe();
    await bridge.openProject();
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("keeps the configured model context window", async () => {
    const bridge = new MemoryDesktopBridge();
    await bridge.saveModelProfile({
      name: "本地模型",
      baseUrl: "http://127.0.0.1:8000/v1",
      model: "local-model",
      dialect: "standard",
      contextWindowTokens: 65_536,
      timeoutMs: 120_000,
      isDefault: true,
    });

    const snapshot = await bridge.load();
    expect(snapshot.modelProfiles[0].contextWindowTokens).toBe(65_536);
  });

  it("rejects a model context window outside the UI safety range", async () => {
    const bridge = new MemoryDesktopBridge();
    await expect(
      bridge.saveModelProfile({
        name: "错误配置",
        baseUrl: "http://127.0.0.1:8000/v1",
        model: "local-model",
        dialect: "standard",
        contextWindowTokens: 4_095,
        timeoutMs: 120_000,
        isDefault: true,
      }),
    ).rejects.toThrow("16384");
  });

  it("rejects an output limit that consumes the agent input budget", async () => {
    const bridge = new MemoryDesktopBridge();
    await expect(
      bridge.saveModelProfile({
        name: "错误配置",
        baseUrl: "http://127.0.0.1:8000/v1",
        model: "local-model",
        dialect: "standard",
        maxOutputTokens: 197_632,
        contextWindowTokens: 198_656,
        timeoutMs: 120_000,
        isDefault: true,
      }),
    ).rejects.toThrow("8192");
  });

  it("keeps memory and local MCP capability changes in the preview bridge", async () => {
    const bridge = new MemoryDesktopBridge();
    await bridge.setTaskMemoryEnabled("task-1", false);
    await bridge.saveLocalMcpServer({
      taskId: "task-1",
      name: "local_docs",
      command: "npx",
      args: ["-y", "@example/docs-server"],
      environmentVariables: ["DOCS_TOKEN"],
      enabled: true,
    });

    let capabilities = await bridge.loadAgentCapabilities("task-1");
    expect(capabilities.memoryEnabled).toBe(false);
    expect(capabilities.mcpServers).toEqual([
      expect.objectContaining({ name: "local_docs", status: "notStarted" }),
    ]);

    await expect(bridge.resetLocalMemory("task-1")).resolves.toBeUndefined();

    await bridge.removeLocalMcpServer("task-1", "local_docs");
    capabilities = await bridge.loadAgentCapabilities("task-1");
    expect(capabilities.mcpServers).toEqual([]);
  });
});
