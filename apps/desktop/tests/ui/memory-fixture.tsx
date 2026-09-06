import { useState } from "react";
import { createRoot } from "react-dom/client";
import { ProjectMemoryPanel } from "../../src/settings/ProjectMemoryPanel";
import { AgentSettings } from "../../src/settings/AgentSettings";
import { MemoryDesktopBridge } from "../../src/bridge/memoryBridge";
import type { DesktopBridge, ProjectMemoryView } from "../../src/bridge/types";
import "../../src/styles.css";

let release: (() => void) | undefined;
let calls = 0;
let resetCalls = 0;
let cleared = false;
const scenario = new URLSearchParams(location.search).get("scenario");
const view = (taskId: string): ProjectMemoryView => ({ taskId, projectPath: `E:/项目-${taskId}`, legacyHistory: true, documents: [
  { name: "MEMORY.md", title: "长期记忆索引", status: "ready", hash: "fixture", content: `记忆-${taskId}\n<script>window.LEAK = true</script>` },
  { name: "memory_summary.md", title: "摘要", status: "missing", hash: null, content: null },
  { name: "raw_memories.md", title: "来源记录", status: "unreadable", hash: null, content: null },
] });
const bridge: DesktopBridge = new MemoryDesktopBridge();
const notes = new Map([ ["A", {content:"A 的已确认约定", revision:1}], ["B", {content:"B 的独立约定", revision:1}] ]);
let noteSaves = 0;
bridge.loadProjectMemoryNotes = async (taskId) => ({taskId, ...notes.get(taskId)!});
bridge.saveProjectMemoryNotes = async (taskId, content, expectedRevision) => {
  noteSaves += 1;
  if (scenario === "notes-save-race") await new Promise<void>((resolve) => {release = resolve;});
  if (scenario === "notes-conflict" && noteSaves === 1) {
    notes.set(taskId, {content:"另一个窗口的最新纠正", revision:2});
    throw new Error("PRIVATE_BACKEND_CONFLICT");
  }
  if (notes.get(taskId)!.revision !== expectedRevision) throw new Error("version conflict");
  const updated = {content, revision:expectedRevision+1}; notes.set(taskId, updated);
  return {taskId, ...updated};
};
if (scenario !== "reset-busy" && scenario !== "forget") bridge.loadAgentCapabilities = async () => { throw new Error("测试内核不可用"); };
bridge.forgetProjectMemory = async () => { cleared = true; return 1788600000; };
bridge.resetLocalMemory = async () => {
  resetCalls += 1;
  if (resetCalls === 1) throw new Error("failed to reset memory under C:/private: memory workspace busy; retry after memory processing finishes");
  cleared = true;
};
bridge.loadProjectMemory = async (taskId: string) => {
  calls += 1;
  if (scenario === "failure" && calls === 1) throw new Error("INTERNAL_SECRET_FIXTURE");
  if (scenario === "race" && taskId === "A") await new Promise<void>((resolve) => { release = resolve; });
  const result = view(scenario === "wrong-task" ? "OTHER" : taskId);
  if (cleared) result.documents = result.documents.map((document) => ({ ...document, status: "missing", hash: null, content: null }));
  return result;
};
function Fixture() {
  const [task, setTask] = useState("A");
  if (scenario === "kernel-failure" || scenario === "reset-busy" || scenario === "forget") return <AgentSettings bridge={bridge} taskId={task} onClose={() => {}} />;
  return <main style={{ maxWidth: 620, margin: "40px auto" }}>
    <button onClick={() => setTask("B")}>切换项目 B</button>
    <button onClick={() => release?.()}>返回旧项目结果</button>
    <ProjectMemoryPanel bridge={bridge} taskId={task} />
  </main>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
