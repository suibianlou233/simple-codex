export type ProjectSummary = {
  id: string;
  name: string;
  path: string;
};

export type TaskStatus = "draft" | "ready" | "running" | "completed" | "failed";

export type PermissionLevel =
  | "approval"
  | "project_full_access"
  | "system_full_access";

export type TaskSummary = {
  id: string;
  projectId: string;
  title: string;
  goal: string;
  status: TaskStatus;
  permissionLevel: PermissionLevel;
  updatedAt: string;
};

export type TimelineEntry = {
  phase?: "commentary" | "final_answer" | null;
  id: string;
  taskId: string;
  turnId?: string | null;
  kind:
    | "user"
    | "assistant"
    | "command"
    | "file_read"
    | "file_change"
    | "approval"
    | "error"
    | "lifecycle";
  title: string;
  detail?: string;
  createdAt: string;
  sequence?: number;
  revision?: number;
  status?: "streaming" | "pending" | "running" | "completed" | "failed" | "cancelled";
  lowValue?: boolean;
};

export type ChildReport = {
  assignments?: Array<{
    senderThreadId: string; senderTurnId: string; itemId: string;
    receivers: string[]; instruction: string; followUp: boolean;
    status: "pending" | "dispatched" | "failed" | "unknown";
  }>;
  outcomes: Array<{ threadId: string; turnId: string | null; status: "completed" | "failed" | "cancelled" | "unknown" }>;
  rejectedOperations: number;
};

export type TurnSummary = {
  childReport?: ChildReport | null;
  id: string;
  taskId: string;
  status: "running" | "completed" | "failed" | "cancelled";
  phase:
    | "preparing_kernel"
    | "preparing"
    | "sampling"
    | "executing_tools"
    | "waiting_approval"
    | "waiting_children"
    | "checking_completion"
    | "checking_submission"
    | "submission_recovery_required"
    | "executing_actions"
    | "completed"
    | "failed"
    | "cancelled";
  startedAt: string;
  finishedAt: string | null;
  sequence: number;
};

export type ModelDialect = "standard" | "deep_seek" | "qwen";

export type ModelProfileSummary = {
  id: string;
  name: string;
  baseUrl: string;
  model: string;
  dialect: ModelDialect;
  maxOutputTokens: number | null;
  contextWindowTokens: number | null;
  timeoutMs: number;
  isDefault: boolean;
  hasCredential: boolean;
};

export type ToolActionStatus =
  | "pending"
  | "running"
  | "applied"
  | "rejected"
  | "failed"
  | "undone";

export type ToolActionSummary = {
  id: string;
  isSubagent?: boolean;
  taskId: string;
  turnId: string;
  kind: "write_file" | "run_command";
  status: ToolActionStatus;
  title: string;
  detail: string;
  diff: string | null;
  result: string | null;
  canUndo: boolean;
  createdAt: string;
  workingDirectory?: string | null;
  requestedCapabilities?: string[];
  risk?: string;
};

export type RuntimeStatus = {
  core: "mock" | "connected" | "unavailable";
  storage: "memory" | "sqlite" | "unavailable";
  network: "offline" | "model-only";
  dataLocation: string;
  workspaceSandboxReady: boolean;
  workspaceSandboxHealth: WorkspaceSandboxHealth;
};

export type WorkspaceSandboxHealth =
  | {
      status: "ready";
      backend: "windows_codex";
      setupVersion: number;
    }
  | {
      status: "needs_setup";
      backend: "windows_codex";
      expectedSetupVersion: number;
    }
  | {
      status: "drifted";
      backend: "windows_codex";
      expectedSetupVersion: number;
      installedSetupVersion: number | null;
    }
  | { status: "unsupported_platform" };

export type ContextUsage = {
  taskId: string;
  estimatedTokens: number;
  contextWindowTokens: number;
  reservedOutputTokens: number;
  messageCount: number;
  toolExchangeCount: number;
};

export type AttachmentSummary = {
  path: string;
  name: string;
  sizeBytes: number;
};

export type DesktopSnapshot = {
  projects: ProjectSummary[];
  tasks: TaskSummary[];
  turns: TurnSummary[];
  timeline: TimelineEntry[];
  modelProfiles: ModelProfileSummary[];
  activeModelProfileId: string | null;
  activeTurnId: string | null;
  actions: ToolActionSummary[];
  contextUsage: ContextUsage[];
  activeProjectId: string | null;
  activeTaskId: string | null;
  taskRevisions: Record<string, number>;
  runtime: RuntimeStatus;
};

export type InspectorFile = {
  path: string;
  content: string;
  sha256: string;
  truncated: boolean;
};

export type GitChangedFile = {
  path: string;
  additions: number;
  deletions: number;
  status: string;
  statisticsAvailable?: boolean;
};

export type GitWorkspaceDiff = {
  supported: boolean;
  summary: string;
  files: GitChangedFile[];
  unifiedDiff: string;
};

export type TerminalSession = {
  id: string;
  taskId: string;
  projectId: string;
  cwd: string;
  processBoundary: "windows_job_object" | "process_only";
  createdAt: string;
};

export type TerminalCommandResult = {
  commandId: string;
  exitCode: number | null;
  stdout: string;
  stderr: string;
  truncated: boolean;
};

export type CreateTaskInput = {
  projectId: string;
  title: string;
  goal: string;
  permissionLevel?: PermissionLevel;
};

export type StartChatResult = {
  taskId: string;
  turnId: string;
};

export type SaveModelProfileInput = {
  profileId?: string;
  name: string;
  baseUrl: string;
  model: string;
  dialect: ModelDialect;
  apiKey?: string;
  maxOutputTokens?: number;
  contextWindowTokens?: number;
  timeoutMs: number;
  isDefault: boolean;
};

export type AgentCapabilities = {
  codeModeAvailable: boolean;
  memoryEnabled: boolean;
  memoryUnavailableReason?: string | null;
  kernelNotice?: string | null;
  skills: Array<{
    name: string;
    description: string;
    scope: string;
    enabled: boolean;
  }>;
  mcpServers: Array<{
    name: string;
    status: string;
    authStatus: string;
    toolCount: number;
  }>;
};

export type ProjectMemoryView = {
  taskId: string;
  projectPath: string;
  legacyHistory: boolean;
  documents: Array<{
    name: string;
    title: string;
    status: "ready" | "missing" | "tooLarge" | "unreadable" | "unsafe";
    content: string | null;
    hash: string | null;
  }>;
};

export type ProjectMemoryNotes = { taskId: string; content: string; revision: number };

export type SaveLocalMcpServerInput = {
  taskId: string;
  name: string;
  command: string;
  args: string[];
  cwd?: string;
  environmentVariables: string[];
  enabled: boolean;
};

export type Unsubscribe = () => void;

export interface DesktopBridge {
  load(): Promise<DesktopSnapshot>;
  installWorkspaceSandbox(): Promise<void>;
  subscribe(listener: (snapshot: DesktopSnapshot) => void): Unsubscribe;
  openProject(): Promise<void>;
  pickAttachments(projectId: string): Promise<AttachmentSummary[]>;
  exportConversation(taskId: string): Promise<string | null>;
  exportDiagnostics(): Promise<string | null>;
  selectProject(projectId: string): Promise<void>;
  createTask(input: CreateTaskInput): Promise<void>;
  selectTask(taskId: string): Promise<void>;
  saveModelProfile(input: SaveModelProfileInput): Promise<void>;
  selectModelProfile(profileId: string): Promise<void>;
  loadAgentCapabilities(taskId: string): Promise<AgentCapabilities>;
  setTaskMemoryEnabled(taskId: string, enabled: boolean): Promise<void>;
  resetLocalMemory(taskId: string): Promise<void>;
  loadProjectMemory(taskId: string): Promise<ProjectMemoryView>;
  loadProjectMemoryNotes(taskId: string): Promise<ProjectMemoryNotes>;
  saveProjectMemoryNotes(taskId: string, content: string, expectedRevision: number): Promise<ProjectMemoryNotes>;
  forgetProjectMemory(taskId: string): Promise<number>;
  saveLocalMcpServer(input: SaveLocalMcpServerInput): Promise<void>;
  removeLocalMcpServer(taskId: string, name: string): Promise<void>;
  startChat(
    projectId: string,
    content: string,
    permissionLevel: PermissionLevel,
  ): Promise<StartChatResult>;
  setTaskPermission(taskId: string, permissionLevel: PermissionLevel): Promise<void>;
  sendMessage(taskId: string, content: string): Promise<void>;
  regenerateResponse(taskId: string): Promise<void>;
  reviseMessage(taskId: string, messageId: string, content: string): Promise<void>;
  branchConversation(taskId: string, messageId: string): Promise<void>;
  cancelTurn(turnId: string): Promise<void>;
  approveAction(actionId: string): Promise<void>;
  rejectAction(actionId: string): Promise<void>;
  undoAction(actionId: string): Promise<void>;
  cancelAction(actionId: string): Promise<void>;
  loadWorkspaceDiff(taskId: string): Promise<GitWorkspaceDiff>;
  readProjectFile(taskId: string, path: string): Promise<InspectorFile>;
  openTerminal(taskId: string): Promise<TerminalSession>;
  runTerminalCommand(
    sessionId: string,
    program: string,
    args: string[],
    cwd?: string,
  ): Promise<TerminalCommandResult>;
}
