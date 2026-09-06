// Dev-only deterministic fixtures. No model, native command, filesystem or MCP access.
import { createRoot } from "react-dom/client";
import { App } from "../../src/App";
import { GUIDE_SEEN_KEY } from "../../src/app/onboarding";
import { CommentaryFixture } from "./CommentaryFixture";
import { OperationOutcomeFixture } from "./OperationOutcomeFixture";
import { MemoryDesktopBridge } from "../../src/bridge/memoryBridge";
import type { DesktopBridge, DesktopSnapshot, GitWorkspaceDiff } from "../../src/bridge/types";
import "../../src/styles.css";
import "../../src/design/workbench.css";

const scenario = new URLSearchParams(location.search).get("scenario");
// Existing workbench cases model a returning user. Onboarding cases use the real first-run path.
if (scenario !== "onboarding") localStorage.setItem(GUIDE_SEEN_KEY, "seen");
const running = scenario === "approval" || scenario === "running" || scenario === "completion-wait" || scenario === "completion-unknown" || scenario === "submission-unknown" || scenario === "submission-cold";
const updatedAt = new Date().toISOString();
const tasks: DesktopSnapshot["tasks"] = [
  { id: "ui-a", projectId: "project-a", title: "让任务切换更加顺手", goal: "优化输入草稿与工作区导航", status: running ? "running" : "completed", permissionLevel: "approval", updatedAt },
  { id: "ui-b", projectId: "project-a", title: "整理项目启动文档", goal: "README 新手说明", status: "completed", permissionLevel: "approval", updatedAt },
  { id: "ui-c", projectId: "project-b", title: "检查配置读取的边界", goal: "配置读取错误信息", status: "failed", permissionLevel: "approval", updatedAt },
];
class FixtureBridge extends MemoryDesktopBridge {
  override async loadWorkspaceDiff(taskId: string): Promise<GitWorkspaceDiff> {
    return { supported: true, summary: `${taskId} · Git 工作区变化，不代表最近一轮修改。`,
      files: [{ path: "src/components/Composer.tsx", additions: 18, deletions: 6, status: "modified" }, { path: "src/app/navigation.ts", additions: 12, deletions: 0, status: "added" }],
      unifiedDiff: "--- before\n+++ after\n-old value\n+new value" };
  }
  override async readProjectFile(taskId: string, path: string) {
    // Intentionally different response speeds to exercise stale-response protection.
    await new Promise((resolve) => setTimeout(resolve, path.includes("Composer") ? 200 : 5));
    return { path, content: `// ${taskId} / ${path}\nexport const draftKey = (projectId, taskId) =>\n  JSON.stringify([projectId, taskId]);`, sha256: "fixture", truncated: false };
  }
}
const bridge: DesktopBridge = new FixtureBridge({
  projects: [{ id: "project-a", name: "simple-code", path: "C:/ui-fixture/simple-code" }, { id: "project-b", name: "playground", path: "C:/ui-fixture/playground" }],
  activeProjectId: "project-a", activeTaskId: scenario === "welcome" ? null : "ui-a", tasks,
  modelProfiles: [{ id: "preview-model", name: "本地预览", model: "mock-model", baseUrl: "http://127.0.0.1/unused", dialect: "standard", maxOutputTokens: 8192, contextWindowTokens: 131072, timeoutMs: 30000, isDefault: true, hasCredential: false }], activeModelProfileId: "preview-model",
  activeTurnId: running ? "ui-turn" : null,
  turns: [{ id: "ui-turn", taskId: "ui-a", status: running ? "running" : "completed", phase: scenario === "submission-unknown" ? "checking_submission" : scenario === "submission-cold" ? "submission_recovery_required" : scenario === "completion-wait" ? "waiting_children" : scenario === "completion-unknown" ? "checking_completion" : running ? "waiting_approval" : "completed", startedAt: updatedAt, finishedAt: running ? null : updatedAt, sequence: 1,
    childReport: scenario === "child-results" ? {assignments:[
      {senderThreadId:"hidden-parent",senderTurnId:"native-turn",itemId:"dispatch-1",receivers:["hidden-child"],instruction:"修复任务搜索的中文输入法组合态，仅修改搜索组件。",followUp:false,status:"dispatched"},
      {senderThreadId:"hidden-parent",senderTurnId:"native-turn",itemId:"dispatch-2",receivers:["hidden-child-2"],instruction:"检查快捷键回归用例，不修改生产代码。",followUp:false,status:"dispatched"},
      {senderThreadId:"hidden-parent",senderTurnId:"native-turn",itemId:"dispatch-3",receivers:[],instruction:"检查使用说明中的手动验证步骤。",followUp:false,status:"failed"},
    ],outcomes:[{threadId:"hidden-child",turnId:"child-turn",status:"failed"},{threadId:"hidden-child-2",turnId:null,status:"unknown"}],rejectedOperations:1} : null }],
  timeline: [
    { id: "ui-message", taskId: "ui-a", turnId: "ui-turn", kind: "user", title: "你", detail: "检查任务切换的体验，先告诉我问题和方案，确认以后再改。", createdAt: updatedAt },
    { id: "ui-read", taskId: "ui-a", turnId: "ui-turn", kind: "file_read", title: "src/components/Composer.tsx", detail: "已读取输入组件与任务选择逻辑。", createdAt: updatedAt },
    { id: "ui-answer", taskId: "ui-a", turnId: "ui-turn", kind: "assistant", title: "Simple", detail: "发现一个会影响连续工作的细节：**切换任务后，输入框还保留着上一项任务的草稿。**\n\n建议按项目和任务分别保存草稿。这样你可以随时切换工作，不必反复复制未发送的内容。\n\n```typescript\nconst key = draftKey(projectId, taskId);\nconst draft = drafts[key] ?? '';\n```\n\n涉及 `Composer.tsx` 与 `navigation.ts`。权限边界与模型调用保持不变。" + (scenario === "long" ? "\n\n```text\n" + "long-content-".repeat(100) + "\n```" : ""), createdAt: updatedAt },
  ],
  actions: scenario === "running" ? [{ id: "ui-running", taskId: "ui-a", turnId: "ui-turn", kind: "run_command", status: "running", title: "读取项目文件", detail: "Get-Content AGENTS.md", diff: null, result: "模拟命令输出", canUndo: false, createdAt: updatedAt }] : scenario === "approval" ? [{ id: "ui-action", taskId: "ui-a", turnId: "ui-turn", kind: "write_file", status: "pending", title: "更新输入草稿逻辑", detail: "src/components/Composer.tsx", diff: "-globalDraft\n+drafts[taskId]", result: null, canUndo: false, createdAt: updatedAt, risk: "修改项目文件" }] : [],
  contextUsage: [{ taskId: "ui-a", estimatedTokens: 16000, contextWindowTokens: 131072, reservedOutputTokens: 8192, messageCount: 8, toolExchangeCount: 2 }],
});
bridge.pickAttachments = async () => {
  if (scenario === "attachment-error") throw new Error("模拟附件读取失败，未访问真实文件");
  return [{ path: "docs/ui-fixture.md", name: "ui-fixture.md", sizeBytes: 128 }];
};
if (scenario === "submission-unknown" || scenario === "submission-cold") {
  // Emulate a stale capability from an old snapshot, not a backend permission.
  const snapshot = await bridge.load();
  snapshot.actions = [{id:"stale-undo",taskId:"ui-a",turnId:"ui-turn",kind:"write_file",status:"applied",title:"旧修改",detail:"file.txt",diff:null,result:null,canUndo:true,createdAt:updatedAt}];
  createRoot(document.getElementById("root")!).render(<App bridge={new MemoryDesktopBridge(snapshot)} />);
} else if (scenario === "operation-transitions") {
  createRoot(document.getElementById("root")!).render(<OperationOutcomeFixture />);
} else if (scenario === "operation-results") {
  const snapshot = await bridge.load();
  snapshot.actions = [{id:"failed-attempt",taskId:"ui-a",turnId:"ui-turn",kind:"run_command",status:"failed",title:"PRIVATE_COMMAND",detail:"PRIVATE_ARGUMENTS",result:"PRIVATE_STACK_TRACE",diff:null,canUndo:false,createdAt:updatedAt}];
  createRoot(document.getElementById("root")!).render(<App bridge={new MemoryDesktopBridge(snapshot)} />);
} else if (scenario === "commentary") {
  createRoot(document.getElementById("root")!).render(<CommentaryFixture />);
} else if (scenario === "configure" || scenario === "onboarding") {
  const snapshot = await bridge.load();
  snapshot.modelProfiles = [];
  snapshot.activeModelProfileId = null;
  snapshot.activeTaskId = null;
  const unconfigured: DesktopBridge = new MemoryDesktopBridge(snapshot);
  unconfigured.pickAttachments = bridge.pickAttachments;
  createRoot(document.getElementById("root")!).render(<App bridge={unconfigured} />);
} else {
  createRoot(document.getElementById("root")!).render(<App bridge={bridge} />);
}
