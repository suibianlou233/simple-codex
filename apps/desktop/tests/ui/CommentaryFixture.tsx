import { useState } from "react";
import { TaskTimeline } from "../../src/items/TaskTimeline";
import type { TimelineEntry, TurnSummary } from "../../src/bridge/types";

export function CommentaryFixture() {
  const [completed, setCompleted] = useState(false);
  const [finalStreaming, setFinalStreaming] = useState(false);
  const entries: TimelineEntry[] = [
    { id: "user", kind: "user" as const, detail: "修复输入法" },
    { id: "note-1", kind: "assistant" as const, phase: "commentary" as const, detail: "正在读取相关文件。" },
    { id: "note-2", kind: "assistant" as const, phase: "commentary" as const, detail: "正在更新输入法处理。" },
    ...(completed || finalStreaming ? [{ id: "answer", kind: "assistant" as const,
      phase: "final_answer" as const, detail: completed ? "修改完成，尚未验证。" : "修改完成，" }] : []),
  ].map((item) => ({ ...item, taskId: "fixture", turnId: "turn", title: item.detail, createdAt: "" }));
  const turns: TurnSummary[] = [{ id: "turn", taskId: "fixture", status: completed ? "completed" : "running",
    phase: completed ? "completed" : "sampling", startedAt: "", finishedAt: completed ? "" : null, sequence: 1 }];
  return <main>
    <button onClick={() => setCompleted(true)}>模拟完成本轮</button>
    <button onClick={() => setFinalStreaming(true)}>模拟开始最终回复</button>
    <TaskTimeline entries={entries} actions={[]} turns={turns} activeTurnId={completed ? null : "turn"}
      disabled={false} onApprove={async () => {}} onReject={async () => {}} onUndo={async () => {}}
      onCancel={async () => true} onRevise={async () => true} onRegenerate={async () => {}} onBranch={async () => {}} />
  </main>;
}
