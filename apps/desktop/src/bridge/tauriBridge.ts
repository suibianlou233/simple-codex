import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AttachmentSummary,
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
  TaskStatus,
  TaskSummary,
  TimelineEntry,
  TurnSummary,
  Unsubscribe,
} from "./types";

type FrontendLogLevel = "info" | "warn" | "error";

export function recordFrontendDiagnostic(
  level: FrontendLogLevel,
  event: string,
  message?: string,
  fields?: Record<string, unknown>,
): void {
  void invoke("record_frontend_log", {
    input: { level, event, message, fields },
  }).catch(() => undefined);
}

async function invokeDesktop<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const started = performance.now();
  try {
    const result = await invoke<T>(command, args);
    recordFrontendDiagnostic("info", "invoke_completed", undefined, {
      command,
      durationMs: Math.round(performance.now() - started),
    });
    return result;
  } catch (error) {
    recordFrontendDiagnostic("error", "invoke_failed", toDiagnosticMessage(error), {
      command,
      durationMs: Math.round(performance.now() - started),
    });
    throw error;
  }
}

function toDiagnosticMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "unknown frontend error";
}

export type BackendProject = {
  id: string;
  name: string;
  path: string;
};

export type BackendTask = {
  id: string;
  projectId: string;
  title: string;
  goal: string;
  status: TaskStatus;
  permissionLevel?: PermissionLevel;
  updatedAtMs: number;
  lastSequence?: number;
};

export type BackendSnapshot = {
  projects: BackendProject[];
  tasks: BackendTask[];
  turns?: Array<{
    childReport?: TurnSummary["childReport"];
    id: string;
    taskId: string;
    status: TurnSummary["status"];
    phase: TurnSummary["phase"];
    startedAtMs: number;
    finishedAtMs: number | null;
    sequence: number;
  }>;
  modelProfiles?: Array<{
    id: string;
    name: string;
    baseUrl: string;
    model: string;
    dialect: "standard" | "deep_seek" | "qwen";
    maxOutputTokens: number | null;
    contextWindowTokens?: number | null;
    timeoutMs: number;
    isDefault: boolean;
    hasCredential: boolean;
  }>;
  activeModelProfileId?: string | null;
  workspaceSandboxReady?: boolean;
  workspaceSandboxHealth?: import("./types").WorkspaceSandboxHealth;
  messages?: Array<{
    phase?: TimelineEntry["phase"];
    id: string;
    taskId: string;
    turnId: string | null;
    role: "user" | "assistant";
    content: string;
    createdAtMs: number;
  }>;
  toolItems?: Array<{
    id: string;
    taskId: string;
    turnId: string | null;
    kind: "command" | "file_read" | "file_change" | "error" | "lifecycle";
    title: string;
    detail: string | null;
    createdAtMs: number;
  }>;
  actions?: Array<{
    id: string;
    isSubagent?: boolean;
    taskId: string;
    turnId: string;
    kind: "write_file" | "run_command";
    status: "pending" | "running" | "applied" | "rejected" | "failed" | "undone";
    title: string;
    detail: string;
    diff: string | null;
    result: string | null;
    canUndo: boolean;
    createdAtMs: number;
  }>;
  contextUsage?: Array<{
    taskId: string;
    estimatedTokens: number;
    contextWindowTokens: number;
    reservedOutputTokens: number;
    messageCount: number;
    toolExchangeCount: number;
  }>;
  dataLocation: string;
};

type BackendAction = NonNullable<BackendSnapshot["actions"]>[number];

type TurnStreamEvent = {
  phase?: TimelineEntry["phase"];
  eventId: string;
  sequence: number;
  turnId: string;
  taskId: string;
  itemId?: string;
  kind:
    | "started"
    | "delta"
    | "reset"
    | "usage"
    | "finished"
    | "failed"
    | "cancelled"
    | "approval_required"
    | "completion_pending"
    | "delegations_updated"
    | "submission_pending";
  content?: string;
  message?: string;
};

export class TauriDesktopBridge implements DesktopBridge {
  private snapshot: DesktopSnapshot | undefined;
  private readonly listeners = new Set<(snapshot: DesktopSnapshot) => void>();
  private eventListener: Promise<void> | undefined;
  private readonly seenEventIds = new Set<string>();
  private readonly pendingDeltas = new Map<string, { payload: TurnStreamEvent; content: string }>();
  private deltaTimer: ReturnType<typeof setTimeout> | undefined;

  async load(): Promise<DesktopSnapshot> {
    this.ensureEventListener();
    return this.accept(await invokeDesktop<BackendSnapshot>("load_snapshot"));
  }

  async installWorkspaceSandbox(): Promise<void> {
    await invokeDesktop("install_workspace_sandbox");
    this.accept(await invokeDesktop<BackendSnapshot>("load_snapshot"));
  }

  subscribe(listener: (snapshot: DesktopSnapshot) => void): Unsubscribe {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async openProject(): Promise<void> {
    this.accept(await invokeDesktop<BackendSnapshot>("open_project"));
  }

  async pickAttachments(projectId: string): Promise<AttachmentSummary[]> {
    return invokeDesktop<AttachmentSummary[]>("pick_attachments", { projectId });
  }

  async exportConversation(taskId: string): Promise<string | null> {
    return invokeDesktop<string | null>("export_conversation", { taskId });
  }

  async exportDiagnostics(): Promise<string | null> {
    return invokeDesktop<string | null>("export_diagnostics");
  }

  async selectProject(projectId: string): Promise<void> {
    const current = this.requireSnapshot();
    if (!current.projects.some((project) => project.id === projectId)) {
      throw new Error("选择的本地项目不存在");
    }
    const activeTaskId = current.tasks.find((task) => task.projectId === projectId)?.id ?? null;
    this.publish({
      ...current,
      activeProjectId: projectId,
      activeTaskId,
      activeTurnId: current.turns.find((turn) => turn.taskId === activeTaskId && turn.status === "running")?.id ?? null,
    });
  }

  async createTask(input: CreateTaskInput): Promise<void> {
    this.accept(await invokeDesktop<BackendSnapshot>("create_task", { input }), input.projectId);
  }

  async selectTask(taskId: string): Promise<void> {
    const current = this.requireSnapshot();
    const task = current.tasks.find((candidate) => candidate.id === taskId);
    if (!task) throw new Error("选择的本地对话不存在");
    this.publish({
      ...current,
      activeProjectId: task.projectId,
      activeTaskId: task.id,
      activeTurnId: current.turns.find((turn) => turn.taskId === task.id && turn.status === "running")?.id ?? null,
    });
  }

  async saveModelProfile(input: SaveModelProfileInput): Promise<void> {
    this.accept(await invokeDesktop<BackendSnapshot>("save_model_profile", { input }));
  }

  async selectModelProfile(profileId: string): Promise<void> {
    this.accept(
      await invokeDesktop<BackendSnapshot>("select_model_profile", { profileId }),
    );
  }

  async loadAgentCapabilities(taskId: string): Promise<AgentCapabilities> {
    return invokeDesktop<AgentCapabilities>("load_agent_capabilities", { taskId });
  }

  async setTaskMemoryEnabled(taskId: string, enabled: boolean): Promise<void> {
    await invokeDesktop("set_task_memory_enabled", { taskId, enabled });
  }

  async resetLocalMemory(taskId: string): Promise<void> {
    await invokeDesktop("reset_local_memory", { taskId });
  }

  async loadProjectMemory(taskId: string): Promise<ProjectMemoryView> {
    return invokeDesktop<ProjectMemoryView>("load_project_memory", { taskId });
  }

  async loadProjectMemoryNotes(taskId: string): Promise<ProjectMemoryNotes> {
    return invokeDesktop<ProjectMemoryNotes>("load_project_memory_notes", { taskId });
  }

  async saveProjectMemoryNotes(taskId: string, content: string, expectedRevision: number): Promise<ProjectMemoryNotes> {
    return invokeDesktop<ProjectMemoryNotes>("save_project_memory_notes", { input: {taskId, content, expectedRevision} });
  }

  async forgetProjectMemory(taskId: string): Promise<number> {
    return invokeDesktop<number>("reset_local_memory", { taskId, mode: "excludePreviousSources" });
  }

  async saveLocalMcpServer(input: SaveLocalMcpServerInput): Promise<void> {
    await invokeDesktop("save_local_mcp_server", { input });
  }

  async removeLocalMcpServer(taskId: string, name: string): Promise<void> {
    await invokeDesktop("remove_local_mcp_server", { taskId, name });
  }

  async startChat(
    projectId: string,
    content: string,
    permissionLevel: PermissionLevel,
  ): Promise<StartChatResult> {
    const profileId = this.requireSnapshot().activeModelProfileId ?? undefined;
    const result = await invokeDesktop<StartChatResult>("start_chat", {
      input: { projectId, profileId, content, permissionLevel },
    });
    const refreshed = this.accept(await invokeDesktop<BackendSnapshot>("load_snapshot"));
    this.publish({
      ...refreshed,
      activeProjectId: projectId,
      activeTaskId: result.taskId,
      activeTurnId: result.turnId,
    });
    return result;
  }

  async setTaskPermission(taskId: string, permissionLevel: PermissionLevel): Promise<void> {
    this.accept(
      await invokeDesktop<BackendSnapshot>("set_task_permission", {
        input: { taskId, permissionLevel },
      }),
    );
  }

  async sendMessage(taskId: string, content: string): Promise<void> {
    const profileId = this.requireSnapshot().activeModelProfileId;
    const turnId = await invokeDesktop<string>("start_turn", {
      input: { taskId, profileId, content },
    });
    const refreshed = this.accept(await invokeDesktop<BackendSnapshot>("load_snapshot"));
    this.publish({ ...refreshed, activeTurnId: turnId });
  }

  async regenerateResponse(taskId: string): Promise<void> {
    const profileId = this.requireSnapshot().activeModelProfileId;
    const turnId = await invokeDesktop<string>("regenerate_response", { taskId, profileId });
    const refreshed = this.accept(await invokeDesktop<BackendSnapshot>("load_snapshot"));
    this.publish({ ...refreshed, activeTaskId: taskId, activeTurnId: turnId });
  }

  async reviseMessage(taskId: string, messageId: string, content: string): Promise<void> {
    const profileId = this.requireSnapshot().activeModelProfileId;
    const turnId = await invokeDesktop<string>("revise_message", {
      taskId,
      messageId,
      content,
      profileId,
    });
    const refreshed = this.accept(await invokeDesktop<BackendSnapshot>("load_snapshot"));
    this.publish({ ...refreshed, activeTaskId: taskId, activeTurnId: turnId });
  }

  async branchConversation(taskId: string, messageId: string): Promise<void> {
    const current = this.requireSnapshot();
    const knownIds = new Set(current.tasks.map((task) => task.id));
    const backend = await invokeDesktop<BackendSnapshot>("branch_conversation", { taskId, messageId });
    const refreshed = this.accept(backend);
    const branch = refreshed.tasks.find((task) => !knownIds.has(task.id));
    if (branch) {
      this.publish({
        ...refreshed,
        activeProjectId: branch.projectId,
        activeTaskId: branch.id,
        activeTurnId: null,
      });
    }
  }

  async cancelTurn(turnId: string): Promise<void> {
    await invokeDesktop("cancel_turn", { turnId });
  }

  async approveAction(actionId: string): Promise<void> {
    try {
      this.accept(await invokeDesktop<BackendSnapshot>("approve_action", { actionId }));
    } catch (error) {
      this.accept(await invokeDesktop<BackendSnapshot>("load_snapshot"));
      throw error;
    }
  }

  async rejectAction(actionId: string): Promise<void> {
    try {
      this.accept(await invokeDesktop<BackendSnapshot>("reject_action", { actionId }));
    } catch (error) {
      this.accept(await invokeDesktop<BackendSnapshot>("load_snapshot"));
      throw error;
    }
  }

  async undoAction(actionId: string): Promise<void> {
    this.accept(await invokeDesktop<BackendSnapshot>("undo_action", { actionId }));
  }

  async cancelAction(actionId: string): Promise<void> {
    await invokeDesktop("cancel_action", { actionId });
  }

  async loadWorkspaceDiff(taskId: string): Promise<GitWorkspaceDiff> {
    return invokeDesktop<GitWorkspaceDiff>("load_workspace_diff", { taskId });
  }



  async readProjectFile(taskId: string, path: string) {
    return invokeDesktop<import("./types").InspectorFile>("read_project_file", { taskId, path });
  }

  async openTerminal(taskId: string): Promise<TerminalSession> {
    return invokeDesktop<TerminalSession>("open_terminal", { taskId });
  }

  async runTerminalCommand(
    sessionId: string,
    program: string,
    args: string[],
    cwd?: string,
  ): Promise<TerminalCommandResult> {
    return invokeDesktop<TerminalCommandResult>("run_terminal_command", {
      input: { sessionId, program, args, cwd },
    });
  }

  private ensureEventListener(): void {
    if (this.eventListener) return;
    const turnListener = listen<TurnStreamEvent>("turn-stream", ({ payload }) => {
      if (this.seenEventIds.has(payload.eventId)) return;
      this.seenEventIds.add(payload.eventId);
      if (this.seenEventIds.size > 2048) {
        const oldest = this.seenEventIds.values().next().value;
        if (oldest) this.seenEventIds.delete(oldest);
      }
      const current = this.snapshot;
      if (!current) return;
      const knownTurn = current.turns.find((turn) => turn.id === payload.turnId);
      if (knownTurn && knownTurn.status !== "running" && ["delta", "reset", "started", "completion_pending", "submission_pending"].includes(payload.kind)) return;
      if (payload.kind === "delta" && payload.content) {
        const streamId = payload.itemId ?? `${payload.turnId}-stream`;
        const pending = this.pendingDeltas.get(streamId);
        this.pendingDeltas.set(streamId, {
          payload,
          content: `${pending?.content ?? ""}${payload.content}`,
        });
        if (!this.deltaTimer) {
          this.deltaTimer = setTimeout(() => this.flushDeltas(), 32);
        }
        return;
      }
      this.flushDeltas();
      const latest = this.snapshot;
      if (!latest) return;
      if (payload.kind === "reset") {
        const streamId = payload.itemId ?? `${payload.turnId}-stream`;
        this.publish({
          ...latest,
          timeline: latest.timeline.filter(
            (entry) => entry.id !== streamId,
          ),
          activeTurnId: payload.taskId === latest.activeTaskId ? payload.turnId : latest.activeTurnId,
        });
        return;
      }
      if (payload.kind === "started") {
        this.publish({ ...latest, activeTurnId: payload.taskId === latest.activeTaskId ? payload.turnId : latest.activeTurnId });
        return;
      }
      if (payload.kind === "completion_pending" || payload.kind === "submission_pending" || payload.kind === "delegations_updated") {
        void invokeDesktop<BackendSnapshot>("load_snapshot").then((backend) => {
          const turn = this.snapshot?.turns.find((turn) => turn.id === payload.turnId);
          if (turn && turn.status !== "running") return;
          this.accept(backend);
        }).catch(() => {
          // Lost status updates never clear an active turn or enable new writes.
        });
        return;
      }
      if (["finished", "failed", "cancelled", "approval_required"].includes(payload.kind)) {
        void invokeDesktop<BackendSnapshot>("load_snapshot")
          .then((backend) => this.accept(backend))
          .catch(() => {
            const fresh = this.snapshot;
            if (payload.message && fresh) {
              this.publish({
                ...fresh,
                activeTurnId: payload.taskId === fresh.activeTaskId && fresh.activeTurnId === payload.turnId
                  && payload.kind !== "approval_required" ? null : fresh.activeTurnId,
                timeline: [
                  ...fresh.timeline,
                  {
                    id: `${payload.turnId}-error`,
                    taskId: payload.taskId,
                    turnId: payload.turnId,
                    kind: "error",
                    title: "回复失败",
                    detail: payload.message,
                    createdAt: new Date().toISOString(),
                    revision: payload.sequence,
                    status: "failed",
                  },
                ],
              });
            }
          });
      }
    });
    const actionListener = listen<string>("action-changed", ({ payload: actionId }) => {
      void invokeDesktop<BackendAction>("load_action", { actionId }).then((action) => {
        const current = this.snapshot;
        if (!current) return;
        const projected = { ...action, createdAt: new Date(action.createdAtMs).toISOString() };
        const exists = current.actions.some((candidate) => candidate.id === action.id);
        this.publish({
          ...current,
          actions: exists
            ? current.actions.map((candidate) => candidate.id === action.id ? projected : candidate)
            : [...current.actions, projected],
        });
      });
    });
    this.eventListener = Promise.all([turnListener, actionListener]).then(() => undefined);
  }

  private flushDeltas(): void {
    if (this.deltaTimer) clearTimeout(this.deltaTimer);
    this.deltaTimer = undefined;
    const current = this.snapshot;
    if (!current || this.pendingDeltas.size === 0) return;
    let timeline = current.timeline;
    let activeTurnId = current.activeTurnId;
    for (const { payload, content } of this.pendingDeltas.values()) {
        const turn = current.turns.find((turn) => turn.id === payload.turnId);
        if (turn && turn.status !== "running") continue;
        const id = payload.itemId ?? `${payload.turnId}-stream`;
        const existing = timeline.find((entry) => entry.id === id);
        if (existing && existing.status !== "streaming") continue;
        timeline = existing
          ? timeline.map((entry) =>
              entry.id === id
                ? { ...entry, phase: payload.phase ?? entry.phase, detail: `${entry.detail ?? ""}${content}`, revision: payload.sequence }
                : entry,
            )
          : [
              ...timeline,
              {
                id,
                taskId: payload.taskId,
                turnId: payload.turnId,
                kind: "assistant" as const,
                phase: payload.phase,
                title: "Agent",
                detail: content,
                createdAt: new Date().toISOString(),
                revision: payload.sequence,
                status: "streaming" as const,
              },
            ];
        if (payload.taskId === current.activeTaskId) activeTurnId = payload.turnId;
    }
    this.pendingDeltas.clear();
    this.publish({ ...current, timeline, activeTurnId });
  }

  private accept(backend: BackendSnapshot, preferredProjectId?: string): DesktopSnapshot {
    const next = projectBackendSnapshot(backend, this.snapshot, preferredProjectId);
    this.publish(next);
    return next;
  }

  private publish(snapshot: DesktopSnapshot): void {
    this.snapshot = snapshot;
    for (const listener of this.listeners) listener(snapshot);
  }

  private requireSnapshot(): DesktopSnapshot {
    if (!this.snapshot) throw new Error("桌面核心尚未完成初始化");
    return this.snapshot;
  }
}

export function projectBackendSnapshot(
  backend: BackendSnapshot,
  previous?: DesktopSnapshot,
  preferredProjectId?: string,
): DesktopSnapshot {
  const projects: ProjectSummary[] = backend.projects.map((project) => ({ ...project }));
  const tasks: TaskSummary[] = backend.tasks.map((task) => ({
    id: task.id,
    projectId: task.projectId,
    title: task.title,
    goal: task.goal,
    status: task.status,
    permissionLevel: task.permissionLevel ?? "approval",
    updatedAt: new Date(task.updatedAtMs).toISOString(),
  }));
  const taskRevisions = Object.fromEntries(
    backend.tasks.map((task) => [task.id, task.lastSequence ?? 0]),
  );
  const activeProjectId = selectExistingId(
    projects,
    preferredProjectId ?? previous?.activeProjectId,
  );
  const activeTaskId = selectActiveTask(tasks, activeProjectId, previous?.activeTaskId);
  const previousTurns = new Map(previous?.turns.map((turn) => [turn.id, turn]));
  const turns = (backend.turns ?? []).map((turn): TurnSummary => {
    const existing = previousTurns.get(turn.id);
    if (existing && existing.sequence > turn.sequence) return existing;
    return {
      childReport: turn.childReport, id: turn.id, taskId: turn.taskId,
      status: turn.status, phase: turn.phase,
      startedAt: new Date(turn.startedAtMs).toISOString(),
      finishedAt: turn.finishedAtMs === null ? null : new Date(turn.finishedAtMs).toISOString(),
      sequence: turn.sequence,
    };
  });
  const runningTurnId = turns
    .filter((turn) => turn.status === "running" && turn.taskId === activeTaskId)
    .sort((left, right) => left.startedAt.localeCompare(right.startedAt))
    .at(-1)?.id ?? null;

  return {
    projects,
    tasks,
    turns,
    timeline: createTimeline(
      tasks,
      backend.messages ?? [],
      backend.toolItems ?? [],
      backend.actions ?? [],
      taskRevisions,
    ),
    modelProfiles: (backend.modelProfiles ?? []).map((profile) => ({
      ...profile,
      contextWindowTokens: profile.contextWindowTokens ?? null,
    })),
    activeModelProfileId: backend.activeModelProfileId ?? null,
    activeTurnId:
      previous?.activeTurnId && turns.some(
        (turn) => turn.id === previous.activeTurnId && turn.taskId === activeTaskId && turn.status === "running",
      )
        ? previous.activeTurnId
        : runningTurnId,
    actions: (backend.actions ?? []).map((action) => ({
      ...action,
      createdAt: new Date(action.createdAtMs).toISOString(),
    })),
    contextUsage: (backend.contextUsage ?? []).map((usage) => ({ ...usage })),
    activeProjectId,
    activeTaskId,
    taskRevisions,
    runtime: {
      core: "connected",
      storage: "sqlite",
      network: (backend.modelProfiles?.length ?? 0) > 0 ? "model-only" : "offline",
      dataLocation: backend.dataLocation,
      workspaceSandboxReady: backend.workspaceSandboxReady ?? false,
      workspaceSandboxHealth: backend.workspaceSandboxHealth ?? {
        status: "unsupported_platform",
      },
    },
  };
}

export function selectExistingId(
  projects: ProjectSummary[],
  preferred: string | null | undefined,
): string | null {
  if (preferred && projects.some((project) => project.id === preferred)) return preferred;
  return projects[0]?.id ?? null;
}

export function selectActiveTask(
  tasks: TaskSummary[],
  projectId: string | null,
  preferred: string | null | undefined,
): string | null {
  if (
    preferred &&
    tasks.some((task) => task.id === preferred && task.projectId === projectId)
  ) {
    return preferred;
  }
  return tasks.find((task) => task.projectId === projectId)?.id ?? null;
}

function createTimeline(
  tasks: TaskSummary[],
  messages: NonNullable<BackendSnapshot["messages"]>,
  toolItems: NonNullable<BackendSnapshot["toolItems"]>,
  actions: NonNullable<BackendSnapshot["actions"]>,
  taskRevisions: Record<string, number>,
): TimelineEntry[] {
  if (messages.length > 0 || toolItems.length > 0) {
    const items: TimelineEntry[] = messages.map((message) => ({
      id: message.id,
      taskId: message.taskId,
      turnId: message.turnId,
      kind: message.role,
      phase: message.phase,
      title: message.role === "user" ? "你" : "Agent",
      detail: message.content,
      createdAt: new Date(message.createdAtMs).toISOString(),
      revision: taskRevisions[message.taskId] ?? 0,
      status: "completed",
    }));
    const visibleToolItems = toolItems.filter((item) => {
      if (item.kind !== "command" && item.kind !== "file_change") return true;
      const actionKind = item.kind === "command" ? "run_command" : "write_file";
      return actions.some((action) =>
        action.taskId === item.taskId
        && action.turnId === item.turnId
        && action.kind === actionKind
        && action.status === "running"
      );
    });
    items.push(...visibleToolItems.map((item) => ({
      id: item.id,
      taskId: item.taskId,
      turnId: item.turnId,
      kind: item.kind,
      title: item.title,
      detail: item.detail ?? undefined,
      createdAt: new Date(item.createdAtMs).toISOString(),
      revision: taskRevisions[item.taskId] ?? 0,
      status: (item.kind === "command" || item.kind === "file_change"
        ? "running"
        : "completed") as TimelineEntry["status"],
      lowValue: true,
    })));
    items.sort((left, right) => left.createdAt.localeCompare(right.createdAt) || left.id.localeCompare(right.id));
    return items;
  }
  return tasks.flatMap((task) => {
    const entries: TimelineEntry[] = [];
    if (task.goal) {
      entries.push({
        id: `${task.id}-goal`,
        taskId: task.id,
        kind: "user",
        title: task.goal,
        createdAt: task.updatedAt,
      });
    }
    entries.push({
      id: `${task.id}-ready`,
      taskId: task.id,
      kind: "lifecycle",
      title: "对话已保存在本机",
      detail: "选择模型后即可开始；文件修改和命令都会先等待你的确认。",
      createdAt: task.updatedAt,
      revision: taskRevisions[task.id] ?? 0,
      status: "completed",
    });
    return entries;
  });
}

export const tauriDesktopBridge = new TauriDesktopBridge();
