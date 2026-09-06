import { describe, expect, it, vi } from "vitest";
import { MemoryDesktopBridge } from "./bridge/memoryBridge";
import { buildConversationTitle, sendConversationMessage } from "./conversation";

describe("chat-first conversation flow", () => {
  it("starts a chat atomically from the first message without the task form", async () => {
    const bridge = new MemoryDesktopBridge();
    await bridge.openProject();
    await bridge.saveModelProfile({
      name: "本地模型",
      baseUrl: "http://127.0.0.1:8000/v1",
      model: "local-model",
      dialect: "standard",
      contextWindowTokens: 32_768,
      timeoutMs: 120_000,
      isDefault: true,
    });
    const project = (await bridge.load()).projects[0];
    const startChat = vi.spyOn(bridge, "startChat");
    const createTask = vi.spyOn(bridge, "createTask");
    const sendMessage = vi.spyOn(bridge, "sendMessage");

    const mode = await sendConversationMessage(
      bridge,
      project.id,
      undefined,
      "检查项目并修复登录表单的校验问题",
      "project_full_access",
    );

    expect(mode).toBe("started");
    expect(startChat).toHaveBeenCalledWith(
      project.id,
      "检查项目并修复登录表单的校验问题",
      "project_full_access",
    );
    expect(createTask).not.toHaveBeenCalled();
    expect(sendMessage).not.toHaveBeenCalled();
    const snapshot = await bridge.load();
    expect(snapshot.tasks).toHaveLength(1);
    expect(snapshot.tasks[0].goal).toBe("检查项目并修复登录表单的校验问题");
    expect(snapshot.tasks[0].permissionLevel).toBe("project_full_access");
    expect(snapshot.timeline[0]).toMatchObject({
      kind: "user",
      detail: "检查项目并修复登录表单的校验问题",
    });
  });

  it("continues an existing conversation through the regular turn command", async () => {
    const bridge = {
      sendMessage: vi.fn(async () => undefined),
      startChat: vi.fn(async () => ({ taskId: "unused", turnId: "unused" })),
    } as unknown as MemoryDesktopBridge;

    const mode = await sendConversationMessage(
      bridge,
      "project-1",
      "task-1",
      "继续检查测试",
      "system_full_access",
    );

    expect(mode).toBe("continued");
    expect(bridge.sendMessage).toHaveBeenCalledWith("task-1", "继续检查测试");
    expect(bridge.startChat).not.toHaveBeenCalled();
  });

  it("derives a bounded single-line title from the first message", () => {
    const title = buildConversationTitle(`  第一行\n${"第二行很长".repeat(12)}  `);
    expect(title).not.toContain("\n");
    expect(Array.from(title).length).toBeLessThanOrEqual(37);
    expect(title.endsWith("…")).toBe(true);
  });
});
