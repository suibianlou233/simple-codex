// Local fixture only: no model requests or native commands.
import { useState } from "react";
import { createRoot } from "react-dom/client";
import { TaskTimeline } from "../../src/items/TaskTimeline";
import type { TimelineEntry, TurnSummary } from "../../src/bridge/types";
import "../../src/styles.css";
import "../../src/design/workbench.css";

function Fixture() {
  const [lines, setLines] = useState(2);
  const [final, setFinal] = useState(false);
  const [status, setStatus] = useState<TurnSummary["status"]>("running");
  const [task, setTask] = useState(1);
  const entry = (id: string, kind: TimelineEntry["kind"], detail: string, phase?: TimelineEntry["phase"]): TimelineEntry => ({
    id: `${task}-${id}`, taskId: `task-${task}`, turnId: `turn-${task}`, kind, detail, title: detail, phase, createdAt: "2026-09-17T08:00:00Z",
  });
  const entries = [entry("user", "user", "帮我优化任务界面的执行过程展示。"),
    entry("progress", "assistant", Array.from({length: lines}, (_, i) => `${i+1}. ${i % 2 ? "正在调整面板布局，保留原有任务状态与操作记录。" : "已读取相关组件，接下来检查流式输出和折叠行为。"}`).join("\n\n"), "commentary"),
    ...(final ? [entry("final", "assistant", "已完成界面改造。执行说明支持折叠，最终回复保持独立显示。", "final_answer")] : []),
  ];
  const turns: TurnSummary[] = [{id: `turn-${task}`, taskId: `task-${task}`, status, phase: status === "running" ? "sampling" : status,
    startedAt: "2026-09-17T08:00:00Z", finishedAt: status === "running" ? null : "2026-09-17T08:00:12Z", sequence: 1}];
  return <main style={{maxWidth: 780, margin: "36px auto", padding: "0 20px"}}>
    <header style={{marginBottom: 28}}><p style={{color:"var(--text-tertiary)",fontSize:12}}>SIMPLE / 界面预览</p><h2>让执行过程更清晰</h2>
      <div style={{display:"flex", flexWrap:"wrap", gap:8}}>
        <button onClick={() => setLines(n=>n+20)}>追加内容</button>
        <button onClick={() => setFinal(true)}>开始最终回复</button>
        <button onClick={() => {setFinal(true);setStatus("completed");}}>完成任务</button>
        <button onClick={() => setStatus("cancelled")}>停止任务</button>
        <button onClick={() => {setTask(n=>n+1);setLines(2);setFinal(false);setStatus("running");}}>切换任务</button>
        <button onClick={() => {document.documentElement.dataset.theme=document.documentElement.dataset.theme === "dark" ? "light" : "dark";}}>切换主题</button>
      </div>
    </header>
    <TaskTimeline entries={entries} actions={[]} turns={turns} activeTurnId={status === "running" ? `turn-${task}` : null}
      disabled={false} onApprove={async()=>{}} onReject={async()=>{}} onUndo={async()=>{}} onCancel={async()=>true}
      onRevise={async()=>true} onRegenerate={async()=>{}} onBranch={async()=>{}}/>
  </main>;
}
createRoot(document.getElementById("root")!).render(<Fixture/>);
