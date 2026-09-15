// Isolated UI fixture: all operations below are in-memory, never native/API calls.
import { createRoot } from "react-dom/client";
import { App } from "../../src/App";
import { GUIDE_SEEN_KEY } from "../../src/app/onboarding";
import { rememberSentText } from "../../src/app/sentText";
import { MemoryDesktopBridge } from "../../src/bridge/memoryBridge";
import type { GitWorkspaceDiff } from "../../src/bridge/types";
import "../../src/styles.css";
import "../../src/design/workbench.css";

const scenario = new URLSearchParams(location.search).get("scenario");
// These regression cases exercise an existing user's workbench, not first-run setup.
localStorage.setItem(GUIDE_SEEN_KEY, "seen");
const time = "2026-09-06T00:00:00.000Z";
const documentBody = "第一章 陨落的天才\n" + "这是需要保留的正文。😀\n".repeat(3000);
const documentInstruction = "请提取前三章并进行分析";
const documentMessage = documentBody + documentInstruction;
class FixtureBridge extends MemoryDesktopBridge {
  private lists = 0;
  private reads = new Map<string, number>();
  private revisions = 0;
  private sends = 0;
  override async sendMessage(taskId: string, content: string) {
    if (scenario !== "paste-send") return super.sendMessage(taskId, content);
    if (++this.sends === 1) throw new Error("fixture send failed");
  }
  override async loadWorkspaceDiff(): Promise<GitWorkspaceDiff> {
    const call = ++this.lists;
    if (scenario === "selection-race" && call === 2) await new Promise((resolve) => setTimeout(resolve, 300));
    return {supported:true,summary:"fixture",unifiedDiff:"fixture diff",
      files: scenario === "remove" && call > 1 ? [] : ["a.ts", "b.ts"].map((path) => ({path,additions:1,deletions:0,status:"M"}))};
  }
  override async readProjectFile(_taskId: string, path: string) {
    const call = (this.reads.get(path) ?? 0) + 1;
    this.reads.set(path, call);
    if (scenario === "late-read" && path === "a.ts" && call === 1) await new Promise((resolve) => setTimeout(resolve, 350));
    if (scenario === "read-error" && call === 2) throw new Error("fixture file unavailable");
    return {path,content:`${path} version ${call}`,sha256:`fixture-${call}`,truncated:false};
  }
  override async reviseMessage() {
    this.revisions++;
    if (this.revisions === 1) throw new Error("fixture send failed");
  }
}
const status = scenario === "uncertain" || scenario === "running" ? "running" : scenario === "cancelled" ? "cancelled" : "completed";
const bridge = new FixtureBridge({
  ...(scenario === "paste-send" ? {activeModelProfileId:"model", modelProfiles:[{
    id:"model",name:"Fixture",baseUrl:"https://example.invalid",model:"fixture",dialect:"standard" as const,
    maxOutputTokens:null,contextWindowTokens:null,timeoutMs:30000,isDefault:true,hasCredential:true,
  }]} : {}),
  projects:[{id:"p",name:"fixture",path:"E:/fixture"}],activeProjectId:"p",activeTaskId:"t",
  tasks:[{id:"t",projectId:"p",title:"前端回归",goal:"fixture",permissionLevel:"approval",status:status === "cancelled" ? "ready" : status,updatedAt:time}],
  activeTurnId: status === "running" ? "turn" : null,
  turns:[{id:"turn",taskId:"t",status,phase:scenario === "uncertain" ? "checking_submission" : status === "running" ? "sampling" : status,startedAt:time,finishedAt:status === "running" ? null : "2026-09-06T00:00:12.000Z",sequence:10}],
  taskRevisions:{t:10},
  timeline:[{id:"user",taskId:"t",turnId:"turn",kind:"user",title:"原始请求",detail:scenario?.startsWith("sent-text") ? documentMessage : "原始请求",createdAt:time},
    {id:"note-1",taskId:"t",turnId:"turn",kind:"assistant",phase:"commentary",title:"进度",detail:"第一条过程说明",createdAt:time},
    {id:"tool",taskId:"t",turnId:"turn",kind:"command",title:"隐藏工具",createdAt:time},
    {id:"note-2",taskId:"t",turnId:"turn",kind:"assistant",phase:"commentary",title:"进度",detail:"第二条过程说明",createdAt:time},
    {id:"answer",taskId:"t",turnId:"turn",kind:"assistant",phase:"final_answer",title:"答复",detail:"最终回答",createdAt:time}],
});
async function mount() {
  if (scenario === "sent-text") await rememberSentText(documentMessage,documentInstruction,[{id:"body",text:documentBody}]);
  createRoot(document.getElementById("root")!).render(<App bridge={bridge} />);
}
void mount();
