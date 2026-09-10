import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  AgentSettings,
  Composer,
  EmptyWorkspace,
  ModelSettings,
  PermissionSelector,
  permissionForNextConversation,
  summarizeWorkProcess,
  TaskTimeline,
  ThemePicker,
  WorkspaceNavigation,
  toMessage,
} from "./App";

describe("toMessage", () => {
  it("renders the local Agent capability entry while native data is loading", () => {
    const bridge = {
      loadAgentCapabilities: async () => ({
        codeModeAvailable: true,
        memoryEnabled: true,
        skills: [],
        mcpServers: [],
      }),
    } as never;
    const markup = renderToStaticMarkup(
      createElement(AgentSettings, {
        bridge,
        taskId: "task-1",
        onClose: () => undefined,
      }),
    );

    expect(markup).toContain("本地 Agent 内核");
    expect(markup).toContain("能力与扩展");
    expect(markup).toContain("暂时无法读取能力");
  });

  it("shows string promise rejections", () => {
    expect(toMessage("后端拒绝了这次操作")).toBe("当前权限不允许这项操作，请检查所选的任务权限。");
  });

  it("uses a safe fallback for unknown rejection values", () => {
    expect(toMessage({ reason: "unknown" })).toBe("这次操作未能完成，请稍后重试。");
  });

  it("uses exact DeepSeek context limits with a safe coding output default", () => {
    const markup = renderToStaticMarkup(
      createElement(ModelSettings, {
        disabled: false,
        onClose: () => undefined,
        onSave: async () => undefined,
      }),
    );

    expect(markup).toContain("上下文窗口（Token）");
    expect(markup).toContain('name="contextWindowTokens"');
    expect(markup).toContain('min="16384"');
    expect(markup).toContain('max="1048576"');
    expect(markup).toContain('value="1048576"');
    expect(markup).toContain('value="https://api.deepseek.com"');
    expect(markup).toContain("单次输出上限（Token）");
    expect(markup).toContain('name="maxOutputTokens"');
    expect(markup).toContain('min="1024"');
    expect(markup).toContain('value="32768"');
  });

  it("blocks a profile that leaves only 1024 tokens for agent input", () => {
    const markup = renderToStaticMarkup(
      createElement(ModelSettings, {
        activeProfile: {
          id: "profile-1",
          name: "DeepSeek",
          baseUrl: "https://api.deepseek.com/v1",
          model: "deepseek-v4-flash",
          dialect: "deep_seek",
          maxOutputTokens: 197_632,
          contextWindowTokens: 198_656,
          timeoutMs: 120_000,
          isDefault: true,
          hasCredential: true,
        },
        disabled: false,
        onClose: () => undefined,
        onSave: async () => undefined,
      }),
    );

    expect(markup).toContain("至少为 Agent 输入保留 8,192 Token");
    expect(markup).toContain('type="submit" disabled=""');
  });

  it("offers a direct light and dark appearance toggle", () => {
    const markup = renderToStaticMarkup(
      createElement(ThemePicker, {
        resolvedTheme: "dark",
        onChange: () => undefined,
      }),
    );
    expect(markup).toContain("浅色");
    expect(markup).toContain("深色");
    expect(markup).toContain("当前为深色主题，切换到浅色主题");
  });

  it("renders only terminal and browser workspace controls", () => {
    const markup = renderToStaticMarkup(
      createElement(WorkspaceNavigation, {
        browserOpen: true,
        terminalOpen: false,
        onBrowserToggle: () => undefined,
        onTerminalToggle: () => undefined,
      }),
    );

    expect(markup).toContain('class="sidebar-workspace-nav"');
    expect(markup).toContain('aria-label="工作区工具"');
    expect(markup).not.toContain("文件");
    expect(markup).not.toContain("Diff");
    expect(markup).not.toContain(">Review<");
    expect(markup).toContain("终端");
    const diffButton = markup.match(/<button[^>]*aria-label="浏览器"[^>]*>/)?.[0];
    expect(diffButton).toContain('class="is-active"');
    expect(diffButton).toContain('aria-pressed="true"');
  });

  it("shows a chat-first welcome without a title or goal form", () => {
    const markup = renderToStaticMarkup(
      createElement(EmptyWorkspace, {
        hasProject: true,
        onOpenProject: () => undefined,
      }),
    );
    expect(markup).toContain("今天，想做点什么？");
    expect(markup).toContain("理解项目");
    expect(markup).toContain("改进代码");
    expect(markup).toContain("检查改动");
    expect(markup).not.toContain("任务名称");
    expect(markup).not.toContain("创建任务");
  });

  it("labels the composer and shows a concise task hint", () => {
    const markup = renderToStaticMarkup(
      createElement(Composer, {
        projectId: "project-1",
        activeTurnId: null,
        disabled: false,
        content: "",
        attachments: [],
        onContentChange: () => undefined,
        onAttach: async () => undefined,
        onRemoveAttachment: () => undefined,
        onSend: async () => undefined,
        onCancel: async () => true,
        permissionLevel: "approval",
        permissionScopeLabel: "用于接下来的新对话",
        permissionDisabled: false,
        workspaceSandboxReady: false,
        onPermissionChange: async () => true,
      }),
    );
    expect(markup).toContain('aria-label="给 Simple 的任务"');
    expect(markup).toContain('aria-describedby="composer-help"');
    expect(markup).toContain("描述你想完成的任务，或添加项目文件…");
    expect(markup).not.toContain("先打开一个本地项目");
    expect(markup).not.toContain("Agent 正在快速回复");
  });

  it.each(["sampling", "checking_submission", "submission_recovery_required"] as const)("omits the status strip while keeping %s locked and stoppable", (phase) => {
    const markup = renderToStaticMarkup(createElement(Composer, {
      projectId:"project", activeTurnId:"turn", phase, disabled:false,content:"",attachments:[],
      onContentChange:()=>undefined,onAttach:async()=>undefined,onRemoveAttachment:()=>undefined,
      onSend:async()=>undefined,onCancel:async()=>true,permissionLevel:"approval",permissionScopeLabel:"当前对话",
      permissionDisabled:false,workspaceSandboxReady:false,onPermissionChange:async()=>true,
    }));
    expect(markup).not.toContain('class="turn-status"');
    expect(markup).not.toContain("正在处理任务");
    expect(markup).toContain('aria-label="停止回复"');
    expect(markup).toMatch(/<textarea[^>]*disabled/);
    expect(markup).toContain(phase === "sampling" ? "任务进行中，完成后可以继续提问" : "任务状态待确认，暂时不能发送新指令");
    expect(markup).not.toContain("任务已完成");
    expect(markup).not.toContain("任务失败");
  });

  it("offers three permission levels and keeps high-permission warnings visible", () => {
    const projectMarkup = renderToStaticMarkup(
      createElement(PermissionSelector, {
        value: "project_full_access",
        disabled: false,
        defaultOpen: true,
        workspaceSandboxReady: true,
        onChange: async () => true,
      }),
    );
    expect(projectMarkup).toContain("逐项确认");
    expect(projectMarkup).toContain("项目自动");
    expect(projectMarkup).toContain("全机自动");
    expect(projectMarkup).toContain("仅在当前项目文件夹内自动操作");
    expect(projectMarkup).toContain('role="radiogroup"');
    expect(projectMarkup).toContain("只影响当前对话");

    const systemMarkup = renderToStaticMarkup(
      createElement(PermissionSelector, {
        value: "system_full_access",
        disabled: true,
        disabledMessage: "任务运行时不可切换",
        workspaceSandboxReady: true,
        onChange: async () => true,
      }),
    );
    expect(systemMarkup).toContain("可操作这台电脑");
    expect(systemMarkup).toContain("任务运行时不可切换");

    const unavailableMarkup = renderToStaticMarkup(
      createElement(PermissionSelector, {
        value: "approval",
        disabled: false,
        defaultOpen: true,
        workspaceSandboxReady: false,
        onChange: async () => true,
      }),
    );
    expect(unavailableMarkup).toContain("沙箱未安装");
    expect(unavailableMarkup).toContain("二级权限需要先安装本机项目沙箱");
  });

  it("keeps an explicitly selected permission when starting the next conversation", () => {
    expect(permissionForNextConversation(false, "system_full_access", "approval"))
      .toBe("system_full_access");
    expect(permissionForNextConversation(true, "approval", "system_full_access"))
      .toBe("system_full_access");
  });

  it("uses the current fast DeepSeek model for a new profile", () => {
    const markup = renderToStaticMarkup(
      createElement(ModelSettings, {
        disabled: false,
        onClose: () => undefined,
        onSave: async () => undefined,
      }),
    );
    expect(markup).toContain('value="deepseek-v4-flash"');
    expect(markup).not.toContain('value="deepseek-chat"');
  });

  it("renders only message content without names, timestamps or bubbles", () => {
    const markup = renderToStaticMarkup(
      createElement(TaskTimeline, {
        entries: [
          { id: "user-1", taskId: "task-1", kind: "user", title: "你", detail: "你好", createdAt: "2026-08-30T12:00:00Z" },
          { id: "agent-1", taskId: "task-1", kind: "assistant", title: "Agent", detail: "**你好**\n\n1. 第一项\n2. 第二项", createdAt: "2026-08-30T12:00:01Z" },
        ],
        actions: [],
        disabled: false,
        onApprove: async () => undefined,
        onReject: async () => undefined,
        onUndo: async () => undefined,
        onCancel: async () => true,
        onRevise: async () => true,
        onRegenerate: async () => undefined,
        onBranch: async () => undefined,
      }),
    );
    expect(markup).toContain("chat-user");
    expect(markup).toContain("chat-assistant");
    expect(markup).toContain("message-text");
    expect(markup).not.toContain("chat-bubble");
    expect(markup).not.toContain("chat-meta");
    expect(markup).not.toContain("Agent</strong>");
    expect(markup).not.toContain("你</strong>");
    expect(markup).not.toContain("<time");
    expect(markup).not.toContain("entry-rail");
    expect(markup).toContain("<strong>你好</strong>");
    expect(markup).toContain("<ol>");
    expect(markup).not.toContain("**你好**");
  });

  it("keeps one collapsed work process per turn and shows approvals separately", () => {
    const markup = renderToStaticMarkup(
      createElement(TaskTimeline, {
        entries: [
          { id: "user-1", taskId: "task-1", turnId: "turn-1", kind: "user", title: "你", detail: "修改项目", createdAt: "2026-08-31T11:59:59Z" },
          { id: "assistant-1", taskId: "task-1", turnId: "turn-1", kind: "assistant", title: "Agent", detail: "已经处理完成", createdAt: "2026-08-31T12:00:03Z" },
        ],
        actions: [
          {
            id: "pending-write",
            taskId: "task-1",
            turnId: "turn-1",
            kind: "write_file",
            status: "pending",
            title: "修改 pending.py",
            detail: "等待写入 pending.py",
            diff: "+pending",
            result: null,
            canUndo: false,
            createdAt: "2026-08-31T12:00:00Z",
          },
          {
            id: "applied-write",
            taskId: "task-1",
            turnId: "turn-1",
            kind: "write_file",
            status: "applied",
            title: "修改 app.py",
            detail: "已写入 app.py",
            diff: "+app",
            result: "完成",
            canUndo: true,
            createdAt: "2026-08-31T12:00:01Z",
          },
          {
            id: "undone-write",
            taskId: "task-1",
            turnId: "turn-1",
            kind: "write_file",
            status: "undone",
            title: "修改 index.html",
            detail: "已撤销 index.html",
            diff: "+html",
            result: "已撤销",
            canUndo: false,
            createdAt: "2026-08-31T12:00:02Z",
          },
        ],
        disabled: false,
        onApprove: async () => undefined,
        onReject: async () => undefined,
        onUndo: async () => undefined,
        onCancel: async () => true,
        onRevise: async () => true,
        onRegenerate: async () => undefined,
        onBranch: async () => undefined,
      }),
    );

    expect(markup.match(/class="work-process work-process-live"/g)).toHaveLength(1);
    expect(markup).not.toContain('<details class="work-process" open="">');
    expect(markup).toContain("有操作需要你确认");
    expect(markup).toContain("撤销修改");
    expect(markup).toContain('class="action-card action-pending"');
    expect(markup).not.toContain("completed-write-actions");
    expect(markup.indexOf("修改项目")).toBeLessThan(markup.indexOf("work-process"));
    expect(markup.indexOf("已经处理完成")).toBeLessThan(markup.indexOf("work-process"));
  });

  it("hides completed read-only tool details without an extra finished status row", () => {
    const markup = renderToStaticMarkup(
      createElement(TaskTimeline, {
        entries: [
          { id: "user-1", taskId: "task-1", turnId: "turn-1", kind: "user", title: "你", detail: "读取项目", createdAt: "2026-09-01T12:00:00Z" },
          { id: "read-1", taskId: "task-1", turnId: "turn-1", kind: "file_read", title: "读取 README.md", detail: "README.md", status: "completed", lowValue: true, createdAt: "2026-09-01T12:00:01Z" },
          { id: "read-2", taskId: "task-1", turnId: "turn-1", kind: "file_read", title: "读取 requirements.md", detail: "docs/requirements.md", status: "completed", lowValue: true, createdAt: "2026-09-01T12:00:02Z" },
          { id: "assistant-1", taskId: "task-1", turnId: "turn-1", kind: "assistant", title: "Agent", detail: "项目说明完成", createdAt: "2026-09-01T12:00:03Z" },
        ],
        turns: [{ id: "turn-1", taskId: "task-1", status: "completed", phase: "completed", startedAt: "2026-09-01T12:00:00Z", finishedAt: "2026-09-01T12:00:03Z", sequence: 1 }],
        actions: [],
        disabled: false,
        onApprove: async () => undefined,
        onReject: async () => undefined,
        onUndo: async () => undefined,
        onCancel: async () => true,
        onRevise: async () => true,
        onRegenerate: async () => undefined,
        onBranch: async () => undefined,
      }),
    );

    expect(markup).not.toContain("work-process-live");
    expect(markup).not.toContain("structured-file_read");
    expect(markup).not.toContain("处理已结束");
    expect(markup).toContain("项目说明完成");
    expect(markup).not.toContain("读取 README.md");
    expect(markup).not.toContain("读取 requirements.md");
  });

  it("groups terminal commands into the same compact work process", () => {
    const markup = renderToStaticMarkup(
      createElement(TaskTimeline, {
        entries: [],
        actions: [
          {
            id: "command-1",
            taskId: "task-1",
            turnId: "turn-1",
            kind: "run_command",
            status: "applied",
            title: "运行终端命令",
            detail: "python -m pytest",
            diff: null,
            result: "3 passed",
            canUndo: false,
            createdAt: "2026-08-31T12:00:00Z",
          },
          {
            id: "command-2",
            taskId: "task-1",
            turnId: "turn-1",
            kind: "run_command",
            status: "failed",
            title: "运行终端命令",
            detail: "python app.py",
            diff: null,
            result: "port already in use",
            canUndo: false,
            createdAt: "2026-08-31T12:00:01Z",
          },
        ],
        disabled: false,
        onApprove: async () => undefined,
        onReject: async () => undefined,
        onUndo: async () => undefined,
        onCancel: async () => true,
        onRevise: async () => true,
        onRegenerate: async () => undefined,
        onBranch: async () => undefined,
      }),
    );

    expect(markup.match(/class="work-process work-process-live"/g)).toHaveLength(1);
    expect(markup).not.toContain("work-process-item");
    expect(markup).toContain("有操作结果需要检查");
    expect(markup).not.toContain("port already in use");
    expect(markup).not.toContain("python app.py");
    expect(markup).not.toContain("command-run-group");
    expect(markup).not.toContain('class="action-card action-applied"');
  });

  it("shows a single live process line before any tool operation exists", () => {
    const markup = renderToStaticMarkup(
      createElement(TaskTimeline, {
        entries: [
          { id: "user-1", taskId: "task-1", turnId: "turn-1", kind: "user", title: "你", detail: "检查项目", createdAt: "2026-08-31T12:00:00Z" },
        ],
        actions: [],
        activeTurnId: "turn-1",
        disabled: false,
        onApprove: async () => undefined,
        onReject: async () => undefined,
        onUndo: async () => undefined,
        onCancel: async () => true,
        onRevise: async () => true,
        onRegenerate: async () => undefined,
        onBranch: async () => undefined,
      }),
    );

    expect(markup.match(/work-process-live/g)).toHaveLength(1);
    expect(markup).toContain("正在处理任务…");
    expect(markup).not.toContain("正在检查项目…");
    expect(markup).not.toContain("action-card");
    expect(summarizeWorkProcess([], true)).toBe("正在处理任务…");
  });
});
