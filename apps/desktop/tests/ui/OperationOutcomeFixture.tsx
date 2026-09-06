import { useState } from "react";
import { TaskTimeline } from "../../src/items/TaskTimeline";
import type { TimelineEntry, ToolActionSummary, TurnSummary } from "../../src/bridge/types";

// UI-only state transitions; no native execution or model requests.
export function OperationOutcomeFixture() {
  const [completed, setCompleted] = useState(false);
  const [settled, setSettled] = useState(false);
  const action = (id: string, status: ToolActionSummary["status"]): ToolActionSummary => ({
    id, status, taskId: "fixture", turnId: "turn", kind: "run_command",
    title: "PRIVATE_COMMAND", detail: "PRIVATE_ARGUMENTS", result: "PRIVATE_STACK_TRACE",
    diff: null, canUndo: false, createdAt: "",
  });
  const entries: TimelineEntry[] = [{id:"user",kind:"user",taskId:"fixture",turnId:"turn",title:"修复问题",createdAt:""},
    ...(completed ? [{id:"answer",kind:"assistant" as const,taskId:"fixture",turnId:"turn",title:"已尝试修复，请验证实际效果。",createdAt:""}] : [])];
  const turn: TurnSummary = {id:"turn",taskId:"fixture",status:completed ? "completed" : "running",phase:completed ? "completed" : "executing_tools",startedAt:"",finishedAt:completed ? "" : null,sequence:1};
  return <main>
    <button onClick={() => setCompleted(true)}>模拟主轮完成</button>
    <button onClick={() => setSettled(true)}>模拟补齐操作记录</button>
    <TaskTimeline entries={entries} actions={[action("attempt", "failed"),action("retry", settled ? "applied" : "running")]}
      turns={[turn]} activeTurnId={settled ? null : "turn"} disabled={false}
      onApprove={async () => {}} onReject={async () => {}} onUndo={async () => {}} onCancel={async () => true}
      onRevise={async () => true} onRegenerate={async () => {}} onBranch={async () => {}} />
  </main>;
}
