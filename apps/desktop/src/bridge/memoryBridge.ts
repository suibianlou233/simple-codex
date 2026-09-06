import type {
  AgentCapabilities,
  ProjectMemoryView,
  ProjectMemoryNotes,
  CreateTaskInput,
  DesktopBridge,
  DesktopSnapshot,
  PermissionLevel,
  ProjectSummary,
  GitWorkspaceDiff,
  SaveModelProfileInput,
  SaveLocalMcpServerInput,
  StartChatResult,
  TerminalCommandResult,
  TerminalSession,
  Unsubscribe,
} from "./types";
import { buildConversationTitle } from "../conversation";

const now = () => new Date().toISOString();

const cloneSnapshot = (snapshot: DesktopSnapshot): DesktopSnapshot =>
  structuredClone(snapshot);

export class MemoryDesktopBridge implements DesktopBridge {
  private snapshot: DesktopSnapshot;
  private readonly listeners = new Set<(snapshot: DesktopSnapshot) => void>();
  private sequence = 0;
  private memoryEnabled = true;
  private readonly projectMemoryNotes = new Map<string, Omit<ProjectMemoryNotes, "taskId">>();
  private readonly localMcpServers = new Map<string, SaveLocalMcpServerInput>();

  constructor(initial?: Partial<DesktopSnapshot>) {
    this.snapshot = {
      projects: [],
      tasks: [],
      turns: [],
      timeline: [],
      modelProfiles: [],
      activeModelProfileId: null,
      activeTurnId: null,
      actions: [],
      contextUsage: [],
      activeProjectId: null,
      activeTaskId: null,
      taskRevisions: {},
      runtime: {
        core: "mock",
        storage: "memory",
        network: "offline",
        dataLocation: "仅在当前应用内存中（等待本地核心接入）",
        workspaceSandboxReady: false,
        workspaceSandboxHealth: { status: "unsupported_platform" },
      },
      ...initial,
    };
  }

  async load(): Promise<DesktopSnapshot> {
    return cloneSnapshot(this.snapshot);
  }

  async installWorkspaceSandbox(): Promise<void> {
    throw new Error("内存预览模式不能安装 Windows 项目沙箱");
  }

  subscribe(listener: (snapshot: DesktopSnapshot) => void): Unsubscribe {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async openProject(): Promise<void> {
    const existing = this.snapshot.projects[0];
    if (existing) {
      this.snapshot.activeProjectId = existing.id;
      this.emit();
      return;
    }

    const project: ProjectSummary = {
      id: this.nextId("project"),
      name: "本地示例项目",
      path: "本地目录 · 等待 Tauri 文件选择器接入",
    };
    this.snapshot.projects.push(project);
    this.snapshot.activeProjectId = project.id;
    this.emit();
  }

  async pickAttachments(): Promise<[]> {
    return [];
  }

  async exportConversation(): Promise<null> {
    return null;
  }

  async exportDiagnostics(): Promise<null> {
    return null;
  }

  async selectProject(projectId: string): Promise<void> {
    this.requireProject(projectId);
    this.snapshot.activeProjectId = projectId;
    const activeTask = this.snapshot.tasks.find(
      (task) => task.id === this.snapshot.activeTaskId,
    );
    if (activeTask?.projectId !== projectId) {
      this.snapshot.activeTaskId =
        this.snapshot.tasks.find((task) => task.projectId === projectId)?.id ?? null;
    }
    this.emit();
  }

  async createTask(input: CreateTaskInput): Promise<void> {
    this.requireProject(input.projectId);
    const title = input.title.trim();
    const goal = input.goal.trim();
    if (!title || !goal) {
      throw new Error("任务名称和目标不能为空");
    }

    const taskId = this.nextId("task");
    const createdAt = now();
    this.snapshot.tasks.unshift({
      id: taskId,
      projectId: input.projectId,
      title,
      goal,
      status: "ready",
      permissionLevel: input.permissionLevel ?? "approval",
      updatedAt: createdAt,
    });
    this.snapshot.activeProjectId = input.projectId;
    this.snapshot.activeTaskId = taskId;
    this.emit();
  }

  async selectTask(taskId: string): Promise<void> {
    const task = this.snapshot.tasks.find((item) => item.id === taskId);
    if (!task) {
      throw new Error(`Unknown task: ${taskId}`);
    }
    this.snapshot.activeTaskId = task.id;
    this.snapshot.activeProjectId = task.projectId;
    this.snapshot.activeTurnId = this.snapshot.turns.find(
      (turn) => turn.taskId === task.id && turn.status === "running",
    )?.id ?? null;
    this.emit();
  }

  async saveModelProfile(input: SaveModelProfileInput): Promise<void> {
    const id = input.profileId ?? this.nextId("model");
    const contextWindowTokens = input.contextWindowTokens ?? 131_072;
    const maxOutputTokens = input.maxOutputTokens ?? null;
    if (contextWindowTokens < 16_384 || contextWindowTokens > 1_048_576) {
      throw new Error("上下文窗口必须在 16384 到 1048576 Token 之间");
    }
    if (maxOutputTokens !== null && maxOutputTokens < 1_024) {
      throw new Error("单次输出上限不能小于 1024 Token");
    }
    if (maxOutputTokens !== null && maxOutputTokens + 8_192 > contextWindowTokens) {
      throw new Error("至少为 Agent 输入保留 8192 Token");
    }
    const profile = {
      id,
      name: input.name,
      baseUrl: input.baseUrl,
      model: input.model,
      dialect: input.dialect,
      maxOutputTokens,
      contextWindowTokens,
      timeoutMs: input.timeoutMs,
      isDefault: true,
      hasCredential: Boolean(input.apiKey) || Boolean(input.profileId),
    };
    this.snapshot.modelProfiles = [
      profile,
      ...this.snapshot.modelProfiles
        .filter((item) => item.id !== id)
        .map((item) => ({ ...item, isDefault: false })),
    ];
    this.snapshot.activeModelProfileId = id;
    this.snapshot.runtime.network = "model-only";
    this.emit();
  }

  async selectModelProfile(profileId: string): Promise<void> {
    if (!this.snapshot.modelProfiles.some((profile) => profile.id === profileId)) {
      throw new Error("选择的模型不存在");
    }
    this.snapshot.modelProfiles = this.snapshot.modelProfiles.map((profile) => ({
      ...profile,
      isDefault: profile.id === profileId,
    }));
    this.snapshot.activeModelProfileId = profileId;
    this.emit();
  }

  async loadAgentCapabilities(_taskId: string): Promise<AgentCapabilities> {
    return {
      codeModeAvailable: false,
      memoryEnabled: this.memoryEnabled,
      skills: [],
      mcpServers: [...this.localMcpServers.values()].map((server) => ({
        name: server.name,
        status: server.enabled ? "notStarted" : "disabled",
        authStatus: "unknown",
        toolCount: 0,
      })),
    };
  }

  async setTaskMemoryEnabled(_taskId: string, enabled: boolean): Promise<void> {
    this.memoryEnabled = enabled;
  }

  async resetLocalMemory(_taskId: string): Promise<void> {}

  async forgetProjectMemory(_taskId: string): Promise<number> {
    throw new Error("预览模式不支持修改持久化记忆来源");
  }

  async loadProjectMemory(taskId: string): Promise<ProjectMemoryView> {
    const task = this.snapshot.tasks.find((item) => item.id === taskId);
    const project = this.snapshot.projects.find((item) => item.id === task?.projectId);
    if (!project) throw new Error("项目不存在");
    return { taskId, projectPath: project.path, legacyHistory: false, documents: [] };
  }

  async loadProjectMemoryNotes(taskId: string): Promise<ProjectMemoryNotes> {
    const task = this.snapshot.tasks.find((item) => item.id === taskId);
    if (!task) throw new Error("任务不存在");
    return {taskId, ...(this.projectMemoryNotes.get(task.projectId) ?? {content:"", revision:0})};
  }

  async saveProjectMemoryNotes(taskId: string, content: string, expectedRevision: number): Promise<ProjectMemoryNotes> {
    const task = this.snapshot.tasks.find((item) => item.id === taskId);
    if (!task) throw new Error("任务不存在");
    const siblings = new Set(this.snapshot.tasks.filter((item) => item.projectId === task.projectId).map((item) => item.id));
    if (this.snapshot.turns.some((turn) => siblings.has(turn.taskId) && turn.status === "running")) throw new Error("项目仍在运行");
    const current = await this.loadProjectMemoryNotes(taskId);
    if (current.revision !== expectedRevision || new TextEncoder().encode(content).length > 8192) throw new Error("版本冲突或内容过长");
    const updated = {taskId, content, revision:current.revision+1};
    this.projectMemoryNotes.set(task.projectId, {content, revision:updated.revision});
    return updated;
  }

  async saveLocalMcpServer(input: SaveLocalMcpServerInput): Promise<void> {
    this.localMcpServers.set(input.name, structuredClone(input));
  }

  async removeLocalMcpServer(_taskId: string, name: string): Promise<void> {
    this.localMcpServers.delete(name);
  }

  async startChat(
    projectId: string,
    content: string,
    permissionLevel: PermissionLevel,
  ): Promise<StartChatResult> {
    this.requireProject(projectId);
    if (!this.snapshot.activeModelProfileId) throw new Error("请先配置模型");
    const message = content.trim();
    if (!message) throw new Error("消息不能为空");

    const taskId = this.nextId("task");
    const turnId = this.nextId("turn");
    const createdAt = now();
    this.snapshot.tasks.unshift({
      id: taskId,
      projectId,
      title: buildConversationTitle(message),
      goal: message,
      status: "running",
      permissionLevel,
      updatedAt: createdAt,
    });
    this.snapshot.timeline.push({
      id: this.nextId("event"),
      taskId,
      turnId,
      kind: "user",
      title: "你",
      detail: message,
      createdAt,
    });
    this.snapshot.taskRevisions[taskId] = 1;
    this.snapshot.turns.push({
      id: turnId,
      taskId,
      status: "running",
      phase: "sampling",
      startedAt: createdAt,
      finishedAt: null,
      sequence: 1,
    });
    this.snapshot.activeProjectId = projectId;
    this.snapshot.activeTaskId = taskId;
    this.snapshot.activeTurnId = turnId;
    this.emit();

    await Promise.resolve();
    this.snapshot.timeline.push({
      id: this.nextId("event"),
      taskId,
      turnId,
      kind: "assistant",
      title: "Agent",
      detail: "浏览器预览模式不会调用真实模型；桌面应用会在这里流式显示回复。",
      createdAt: now(),
    });
    const task = this.snapshot.tasks.find((candidate) => candidate.id === taskId);
    if (task) task.status = "completed";
    const turn = this.snapshot.turns.find((candidate) => candidate.id === turnId);
    if (turn) {
      turn.status = "completed";
      turn.phase = "completed";
      turn.finishedAt = now();
      turn.sequence = 2;
    }
    this.snapshot.taskRevisions[taskId] = 2;
    this.snapshot.activeTurnId = null;
    this.emit();
    return { taskId, turnId };
  }

  async setTaskPermission(taskId: string, permissionLevel: PermissionLevel): Promise<void> {
    const task = this.snapshot.tasks.find((item) => item.id === taskId);
    if (!task) throw new Error("任务不存在");
    if (this.snapshot.activeTurnId && this.snapshot.activeTaskId === taskId) {
      throw new Error("任务运行时不能切换权限");
    }
    task.permissionLevel = permissionLevel;
    task.updatedAt = now();
    this.emit();
  }

  async sendMessage(taskId: string, content: string): Promise<void> {
    if (!this.snapshot.activeModelProfileId) throw new Error("请先配置模型");
    const task = this.snapshot.tasks.find((item) => item.id === taskId);
    if (!task) throw new Error("任务不存在");
    const turnId = this.nextId("turn");
    const createdAt = now();
    task.status = "running";
    this.snapshot.activeTurnId = turnId;
    this.snapshot.timeline.push({
      id: this.nextId("event"),
      taskId,
      turnId,
      kind: "user",
      title: "你",
      detail: content.trim(),
      createdAt,
    });
    const revision = (this.snapshot.taskRevisions[taskId] ?? 0) + 1;
    this.snapshot.taskRevisions[taskId] = revision;
    this.snapshot.turns.push({
      id: turnId,
      taskId,
      status: "running",
      phase: "sampling",
      startedAt: createdAt,
      finishedAt: null,
      sequence: revision,
    });
    this.emit();
    await Promise.resolve();
    this.snapshot.timeline.push({
      id: this.nextId("event"),
      taskId,
      turnId,
      kind: "assistant",
      title: "Agent",
      detail: "浏览器预览模式不会调用真实模型；桌面应用会在这里流式显示回复。",
      createdAt: now(),
    });
    task.status = "completed";
    const turn = this.snapshot.turns.find((candidate) => candidate.id === turnId);
    if (turn) {
      turn.status = "completed";
      turn.phase = "completed";
      turn.finishedAt = now();
      turn.sequence += 1;
      this.snapshot.taskRevisions[taskId] = turn.sequence;
    }
    this.snapshot.activeTurnId = null;
    this.emit();
  }

  async regenerateResponse(taskId: string): Promise<void> {
    const task = this.snapshot.tasks.find((item) => item.id === taskId);
    if (!task) throw new Error("任务不存在");
    const lastUser = [...this.snapshot.timeline]
      .reverse()
      .find((entry) => entry.taskId === taskId && entry.kind === "user");
    if (!lastUser) throw new Error("没有可重新生成的消息");
    const index = this.snapshot.timeline.findIndex((entry) => entry.id === lastUser.id);
    this.snapshot.timeline.splice(index);
    await this.sendMessage(taskId, lastUser.detail ?? lastUser.title);
  }

  async reviseMessage(taskId: string, messageId: string, content: string): Promise<void> {
    const index = this.snapshot.timeline.findIndex(
      (entry) => entry.id === messageId && entry.taskId === taskId && entry.kind === "user",
    );
    if (index < 0) throw new Error("消息不存在");
    this.snapshot.timeline.splice(index);
    await this.sendMessage(taskId, content);
  }

  async branchConversation(taskId: string, messageId: string): Promise<void> {
    const source = this.snapshot.tasks.find((task) => task.id === taskId);
    if (!source) throw new Error("任务不存在");
    const through = this.snapshot.timeline.findIndex((entry) => entry.id === messageId);
    if (through < 0) throw new Error("消息不存在");
    const branchId = this.nextId("task");
    this.snapshot.tasks.unshift({
      ...source,
      id: branchId,
      title: `${source.title} · 分支`,
      status: "ready",
      updatedAt: now(),
    });
    const copied = this.snapshot.timeline
      .slice(0, through + 1)
      .filter((entry) => entry.taskId === taskId)
      .map((entry) => ({ ...entry, id: this.nextId("event"), taskId: branchId }));
    this.snapshot.timeline.push(...copied);
    this.snapshot.activeTaskId = branchId;
    this.snapshot.activeProjectId = source.projectId;
    this.emit();
  }

  async cancelTurn(turnId: string): Promise<void> {
    if (this.snapshot.activeTurnId !== turnId) return;
    this.snapshot.activeTurnId = null;
    const task = this.snapshot.tasks.find((item) => item.id === this.snapshot.activeTaskId);
    if (task) task.status = "ready";
    const turn = this.snapshot.turns.find((candidate) => candidate.id === turnId);
    if (turn) {
      turn.status = "cancelled";
      turn.phase = "cancelled";
      turn.finishedAt = now();
      turn.sequence += 1;
      this.snapshot.taskRevisions[turn.taskId] = turn.sequence;
    }
    this.emit();
  }

  async approveAction(actionId: string): Promise<void> {
    const action = this.snapshot.actions.find((item) => item.id === actionId);
    if (!action || action.status !== "pending") throw new Error("操作已不再等待确认");
    action.status = "applied";
    action.result = "浏览器预览模式：操作已模拟执行";
    action.canUndo = action.kind === "write_file";
    this.emit();
  }

  async rejectAction(actionId: string): Promise<void> {
    const action = this.snapshot.actions.find((item) => item.id === actionId);
    if (!action || action.status !== "pending") throw new Error("操作已不再等待确认");
    action.status = "rejected";
    action.result = "用户拒绝了这项操作";
    this.emit();
  }

  async undoAction(actionId: string): Promise<void> {
    const action = this.snapshot.actions.find((item) => item.id === actionId);
    if (!action?.canUndo) throw new Error("这项操作不能撤销");
    action.status = "undone";
    action.canUndo = false;
    action.result = "修改已撤销";
    this.emit();
  }

  async cancelAction(actionId: string): Promise<void> {
    const action = this.snapshot.actions.find((item) => item.id === actionId);
    if (action?.status === "running") {
      action.status = "failed";
      action.result = "命令已停止";
      this.emit();
    }
  }

  async loadWorkspaceDiff(_taskId: string): Promise<GitWorkspaceDiff> {
    return {
      supported: false,
      summary: "浏览器预览模式未连接本地 Git 服务",
      files: [],
      unifiedDiff: "",
    };
  }

  async readProjectFile(taskId: string, path: string) {
    if (!this.snapshot.tasks.some((task) => task.id === taskId)) throw new Error("任务不存在");
    return { path, content: "浏览器预览模式未连接本地文件服务。", sha256: "", truncated: false };
  }



  async openTerminal(taskId: string): Promise<TerminalSession> {
    const task = this.snapshot.tasks.find((candidate) => candidate.id === taskId);
    if (!task) throw new Error("任务不存在");
    const project = this.snapshot.projects.find((candidate) => candidate.id === task.projectId);
    return {
      id: this.nextId("terminal"),
      taskId,
      projectId: task.projectId,
      cwd: project?.path ?? ".",
      processBoundary: "process_only",
      createdAt: now(),
    };
  }

  async runTerminalCommand(
    _sessionId: string,
    program: string,
    args: string[],
  ): Promise<TerminalCommandResult> {
    return {
      commandId: this.nextId("command"),
      exitCode: 0,
      stdout: `浏览器预览：${[program, ...args].join(" ")}`,
      stderr: "",
      truncated: false,
    };
  }

  private requireProject(projectId: string): void {
    if (!this.snapshot.projects.some((project) => project.id === projectId)) {
      throw new Error(`Unknown project: ${projectId}`);
    }
  }

  private nextId(prefix: string): string {
    this.sequence += 1;
    return `${prefix}-${this.sequence}`;
  }

  private emit(): void {
    const next = cloneSnapshot(this.snapshot);
    for (const listener of this.listeners) {
      listener(next);
    }
  }
}

export const desktopBridge: DesktopBridge = new MemoryDesktopBridge();
