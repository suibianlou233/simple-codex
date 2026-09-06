import type { ChildReport } from "../bridge/types";

export function SubagentReport({ report, ended }: { report: ChildReport; ended: boolean }) {
  const assignments = report.assignments ?? [];
  // Keep spawn order stable even when final observations arrive in another order.
  const ids = [...new Set([...assignments.flatMap((item) => item.receivers), ...report.outcomes.map((child) => child.threadId)])];
  const label = (id: string) => `子任务 ${ids.indexOf(id) + 1}`;
  if (!assignments.length && !report.outcomes.length && !report.rejectedOperations) return null;
  return <details className="subagent-results">
    <summary>{assignments.length ? `子任务分工与结果 · ${assignments.length} 次派发` : `已发现的子任务结果 · ${ids.length} 项`}</summary>
    {assignments.length ? <ol className="subagent-assignments">{assignments.map((item) => <li key={JSON.stringify([item.senderThreadId, item.senderTurnId, item.itemId])}>
      <div>{item.receivers.length ? item.receivers.map(label).join("、") : "新子任务"} · {item.followUp ? "追加指令" : "派发任务"} · {({pending: ended ? "派发结果未确认" : "等待派发回执", dispatched: "已派发", failed: "未能派发", unknown: "派发结果未确认"})[item.status]}</div>
      <p className="subagent-instruction">{item.instruction || "未提供分工说明"}</p>
    </li>)}</ol> : null}
    <ul>{report.outcomes.map((child) => <li key={child.threadId}>{label(child.threadId)}：{({completed: "已完成", failed: "失败", cancelled: "已停止", unknown: "结果未知"} as const)[child.status]}</li>)}</ul>
    {assignments.length && !report.outcomes.length ? <p>尚未取得子任务最终结果。已派发不代表已完成。</p> : null}
    {report.rejectedOperations > 0 ? <p>{report.rejectedOperations} 项子任务操作未获批准，请核对实际修改。</p> : null}
    <p>这里显示已发现子任务最近一次执行的状态，不代表代码已经通过测试。</p>
  </details>;
}
