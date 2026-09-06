import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { TaskTimeline } from "./TaskTimeline";
import type { TimelineEntry, TurnSummary, ToolActionSummary } from "../bridge/types";

const entry = (id: string, kind: TimelineEntry["kind"] = "assistant", turnId = "turn-1"): TimelineEntry =>
  ({ id, kind, taskId: "task-1", turnId, title: id, detail: id, createdAt: "2026-09-05T00:00:00Z" });
const turn = (status: TurnSummary["status"]): TurnSummary =>
  ({ id: "turn-1", taskId: "task-1", status, phase: status === "running" ? "sampling" : status,
    startedAt: "", finishedAt: status === "running" ? null : "", sequence: 1 });
function render(entries: TimelineEntry[], turns: TurnSummary[], activeTurnId?: string, actions: ToolActionSummary[] = []) {
  return renderToStaticMarkup(createElement(TaskTimeline, {
    entries, turns, activeTurnId, actions, disabled: false,
    onApprove: async () => {}, onReject: async () => {}, onUndo: async () => {},
    onCancel: async () => true, onRevise: async () => true, onRegenerate: async () => {}, onBranch: async () => {},
  }));
}
describe("user-facing conversation", () => {
  it.each(["completed", "failed", "cancelled"] as const)("folds %s commentary across hidden tools with the terminal notice outside", (status) => {
    const html = render([entry("request","user"),
      {...entry("first-note"),phase:"commentary"}, entry("hidden-tool","command"),
      {...entry("second-note"),phase:"commentary"}, entry("hidden-lifecycle","lifecycle"),
      {...entry("third-note"),phase:"commentary"}, {...entry("answer"),phase:"final_answer"}], [turn(status)]);
    expect(html.match(/class="completed-commentary"/g)).toHaveLength(1);
    expect(html).toContain("执行过程 · 3 条说明");
    expect(html).not.toContain('class="completed-commentary" open');
    expect(html).not.toContain("hidden-tool");
    expect(html.indexOf("third-note")).toBeLessThan(html.indexOf("</details>"));
    if (status === "completed") expect(html.indexOf("answer")).toBeGreaterThan(html.indexOf("</details>"));
    else expect(html.indexOf(status === "failed" ? "这次任务未能完成" : "任务已停止")).toBeGreaterThan(html.indexOf("</details>"));
  });
  it("does not merge folded notes across distinct turns", () => {
    const html = render([{...entry("note-a"),phase:"commentary"},
      {...entry("note-b","assistant","turn-2"),phase:"commentary"}],
      [turn("completed"),{...turn("cancelled"),id:"turn-2"}]);
    expect(html.match(/class="completed-commentary"/g)).toHaveLength(2);
    expect(html).toContain("任务已停止");
  });
  it("anchors two old cancelled text turns before the latest conversation instead of at the footer", () => {
    const html = render([
      entry("cancelled-request-1", "user", "old-1"),
      entry("cancelled-request-2", "user", "old-2"),
      entry("latest-request", "user"), entry("latest-reply"),
    ], [{...turn("cancelled"), id:"old-1"}, {...turn("cancelled"), id:"old-2"}, turn("completed")]);
    expect(html.match(/任务已停止/g)).toHaveLength(2);
    expect(html.indexOf('data-turn-id="old-1"')).toBeGreaterThan(html.indexOf("cancelled-request-1"));
    expect(html.indexOf('data-turn-id="old-1"')).toBeLessThan(html.indexOf("cancelled-request-2"));
    expect(html.indexOf('data-turn-id="old-2"')).toBeLessThan(html.indexOf("latest-request"));
    expect(html.lastIndexOf("任务已停止")).toBeLessThan(html.indexOf("latest-reply"));
  });
  it("does not use a hidden cancelled reply or tool row as the status anchor", () => {
    const html = render([
      entry("old-request", "user", "old"), entry("hidden-reply", "assistant", "old"),
      entry("hidden-tool", "command", "old"), entry("latest-request", "user"),
    ], [{...turn("cancelled"), id:"old"}, turn("running")], "turn-1");
    expect(html.match(/任务已停止/g)).toHaveLength(1);
    expect(html.indexOf("任务已停止")).toBeLessThan(html.indexOf("latest-request"));
    expect(html).toContain("正在处理任务");
    expect(html).not.toContain("hidden-reply");
  });
  it("does not append off-page cancelled history underneath the current page", () => {
    const entries = [entry("old-request", "user", "old"),
      ...Array.from({length:200}, (_, index) => entry(`current-${index}`, "user"))];
    const html = render(entries, [{...turn("cancelled"), id:"old"}, turn("completed")]);
    expect(html).toContain("显示更早的");
    expect(html).not.toContain("任务已停止");
  });
  it("keeps a current cancelled notice at its own message exactly once", () => {
    const html = render([entry("request", "user")], [turn("cancelled")]);
    expect(html.match(/任务已停止/g)).toHaveLength(1);
    expect(html).toContain("停止不会自动撤销已经完成的修改");
  });
  it("keeps live work visible when its message has not arrived", () => {
    const html = render([], [turn("running")], "turn-1");
    expect(html).toContain("正在处理任务");
    expect(html.match(/data-turn-id="turn-1"/g)).toHaveLength(1);
  });
  it("shows preparation as pending work instead of pretending the model is executing", () => {
    const html = render([entry("request", "user")], [{...turn("running"), phase:"preparing_kernel"}], "turn-1");
    expect(html).toContain("正在连接执行内核，可停止…");
    expect(html).not.toContain("处理已结束");
  });
  it.each(["pending", "failed", "unknown", "dispatched"] as const)("does not treat a %s dispatch receipt as a completed child", (status) => {
    const root = {...turn("completed"), childReport:{outcomes:[], rejectedOperations:0, assignments:[{
      senderThreadId:"PRIVATE_SENDER", senderTurnId:"PRIVATE_TURN", itemId:"PRIVATE_ITEM", receivers: status === "dispatched" ? ["PRIVATE_CHILD"] : [],
      instruction:"检查组合态 <script>unsafe()</script>", followUp:false, status,
    }]}};
    const html = render([entry("request", "user"),entry("root-final")],[root]);
    expect(html).toContain("有子任务结果需要检查");
    expect(html).toContain("子任务分工与结果");
    expect(html).toContain("已派发不代表已完成");
    expect(html).not.toContain("PRIVATE_");
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("等待派发回执");
    expect(html).not.toContain('<details class="subagent-results" open');
  });
  it("links final child states to stable dispatch order and keeps followups distinct", () => {
    const assignments = ["z-child", "a-child", "z-child"].map((id,index) => ({senderThreadId:"parent",senderTurnId:"native",itemId:String(index),receivers:[id],instruction:`分工${index}`,followUp:index === 2,status:"dispatched" as const}));
    const root = {...turn("completed"),childReport:{assignments,rejectedOperations:0,outcomes:[{threadId:"a-child",turnId:null,status:"failed" as const},{threadId:"z-child",turnId:null,status:"completed" as const}]}};
    const html = render([entry("request","user")],[root]);
    expect(html).toContain("子任务 2：失败");
    expect(html).toContain("子任务 1：已完成");
    expect(html).toContain("追加指令");
    expect(html).not.toContain("z-child");
  });
  const applied: ToolActionSummary = {
    id: "old-write", taskId: "task-1", turnId: "turn-1", kind: "write_file", status: "applied",
    title: "修改文件", detail: "file.txt", diff: "-before\n+after", result: null,
    canUndo: true, createdAt: "",
  };
  it.each(["sampling", "checking_submission", "submission_recovery_required", "waiting_children", "checking_completion"] as const)("hides cached undo controls while a later turn is %s", (phase) => {
    const current = {...turn("running"), id: "turn-2", phase};
    const html = render([entry("old", "user"), entry("new", "user", "turn-2")], [turn("completed"), current], undefined, [applied]);
    expect(html).not.toContain("撤销修改");
  });
  it("hides cached undo until an active turn without history has been resolved", () => {
    expect(render([entry("old", "user")], [turn("completed")], "missing-turn", [applied])).not.toContain("撤销修改");
  });
  it.each(["completed", "cancelled", "failed"] as const)("places %s elapsed time inside the process summary, not under the answer", (status) => {
    const root = {
      ...turn(status),
      startedAt: "2026-09-05T00:00:00Z",
      finishedAt: "2026-09-05T00:00:12Z",
    };
    const html = render(
      [entry("request", "user"), {...entry("progress"),phase:"commentary"}, entry("tool", "file_read"), entry("final", "assistant")],
      [root],
    );
    expect(html).toContain("本轮用时 00:12");
    expect(html.indexOf("本轮用时")).toBeGreaterThan(html.indexOf("<summary>执行过程"));
    expect(html.indexOf("本轮用时")).toBeLessThan(html.indexOf("</summary>"));
    expect(html.match(/class="turn-elapsed"/g)).toHaveLength(1);
    expect(html).not.toContain("正在处理任务");
  });
  it("does not invent an elapsed time when a folded turn has no end timestamp", () => {
    const html = render([{...entry("progress"),phase:"commentary"}], [{...turn("completed"),finishedAt:null}]);
    expect(html).toContain("执行过程");
    expect(html).not.toContain("本轮用时");
  });
  it("omits the elapsed label for completed turns without tool work", () => {
    const root = {
      ...turn("completed"),
      startedAt: "2026-09-05T00:00:00Z",
      finishedAt: "2026-09-05T00:00:12Z",
    };
    const html = render([entry("request", "user"), entry("final", "assistant")], [root]);
    expect(html).not.toContain("本轮用时");
  });
  it("restores undo after confirmed completion even if an obsolete active id remains", () => {
    expect(render([entry("old", "user")], [turn("completed")], "turn-1", [applied])).toContain("撤销修改");
  });
  it.each(["checking_submission","submission_recovery_required"] as const)("does not pretend %s is active work or expose stale approval controls", (phase) => {
    const root = {...turn("running"), phase};
    const html = render([entry("request","user")],[root],root.id);
    expect(html).not.toContain("任务执行状态待确认");
    expect(html).not.toContain('class="work-process work-process-live"');
    expect(html).not.toContain("正在处理任务");
    expect(html).not.toContain("处理已结束");
  });
  it.each(["failed", "cancelled", "unknown"] as const)("shows a completed root with a %s child as needing review", (status) => {
    const root = {...turn("completed"),childReport:{outcomes:[{threadId:"PRIVATE_THREAD_ID",turnId:"child-turn",status}],rejectedOperations:0}};
    const html = render([entry("request","user"),entry("root-final")],[root]);
    expect(html).toContain("有子任务结果需要检查");
    expect(html).toContain("已发现的子任务结果");
    expect(html).not.toContain("PRIVATE_THREAD_ID");
    expect(html).not.toContain('<details class="subagent-results" open');
    expect(html).toContain("root-final");
  });
  it("distinguishes child completion from a rejected child operation", () => {
    const root = {...turn("completed"),childReport:{outcomes:[{threadId:"child",turnId:"child-turn",status:"completed" as const}],rejectedOperations:1}};
    const html = render([entry("request","user")],[root]);
    expect(html).toContain("有子任务结果需要检查");
    expect(html).toContain("项子任务操作未获批准");
  });
  it("identifies a child approval without approving it or exposing execution results", () => {
    const action: ToolActionSummary = {id:"child-action",isSubagent:true,taskId:"task-1",turnId:"turn-1",kind:"write_file",status:"pending",title:"修改文件",detail:"fixture.txt",diff:"-before\n+after",result:"PRIVATE_RESULT",canUndo:false,createdAt:new Date(0).toISOString()};
    const html = render([entry("request","user")],[turn("running")],"turn-1",[action]);
    expect(html).toContain("子任务需要你确认");
    expect(html).toContain("应用修改");
    expect(html).toContain("拒绝");
    expect(html).not.toContain("PRIVATE_RESULT");
  });
  it("streams final replies beside live commentary and folds commentary after completion", () => {
    const messages: TimelineEntry[] = [entry("request", "user"),
      {...entry("intermediate-note"), phase: "commentary"},
      {...entry("final streaming text"), phase: "final_answer", status: "streaming"}];
    const html = render(messages, [turn("running")], "turn-1");
    expect(html).toContain("final streaming text");
    expect(html).toContain("intermediate-note");
    expect(html).not.toContain("completed-commentary");
    expect(html).toContain("正在处理任务");
    const completed = render(messages, [turn("completed")]);
    expect(completed).toContain("执行过程 · 1 条说明");
    expect(completed.indexOf("intermediate-note")).toBeLessThan(completed.indexOf("</details>"));
    expect(completed.indexOf("final streaming text")).toBeGreaterThan(completed.indexOf("</details>"));
    expect(render(messages, [turn("failed")])).not.toContain("final streaming text");
  });
  it("keeps commentary-only completion inside a closed process disclosure", () => {
    const html = render([{...entry("interim only"), phase: "commentary"}], [turn("completed")]);
    expect(html).toContain("执行过程 · 1 条说明");
    expect(html.indexOf("interim only")).toBeLessThan(html.indexOf("</details>"));
    expect(html).not.toContain('class="completed-commentary" open');
  });
  const entries = [entry("request", "user"), entry("read-note"), entry("edit-note"), entry("final-answer")];
  it("shows the final answer, never exposes completed troubleshooting messages", () => {
    const html = render(entries, [turn("completed")]);
    expect(html).toContain("final-answer");
    expect(html).not.toContain("read-note");
    expect(html).not.toContain("edit-note");
    expect(html).not.toContain("completed-commentary");
  });
  it("shows neutral progress rather than intermediate model prose", () => {
    const html = render(entries, [turn("running")], "turn-1");
    expect(html).toContain("正在处理任务");
    expect(html).not.toContain("read-note");
    expect(html).not.toContain("final-answer");
  });
  it.each(["failed", "cancelled"] as const)("keeps %s truthful without exposing troubleshooting prose", (status) => {
    const html = render([...entries, entry("502 http://localhost:1234/secret", "error")], [turn(status)]);
    expect(html).toContain(status === "failed" ? "这次任务未能完成" : "任务已停止");
    expect(html).not.toContain("localhost");
    expect(html).not.toContain("read-note");
    expect(html).not.toContain("final-answer");
  });
  it("retains unknown historical last replies without guessing from their text", () => {
    expect(render([entry("TypeError is a JavaScript error")], [])).toContain("TypeError is a JavaScript error");
    expect(render([entry("final-answer")], [turn("completed")], "turn-1")).toContain("final-answer");
  });
  it("does not redact user requests or unlinked ordinary answers", () => {
    const html = render([{ ...entry("user debug request", "user"), turnId: null },
      { ...entry("ordinary answer"), turnId: null }], []);
    expect(html).toContain("user debug request");
    expect(html).toContain("ordinary answer");
  });
  it("sanitizes orphan failures, with no raw details in the DOM", () => {
    const html = render([{ ...entry("stack trace secret-value", "error"), turnId: null }], []);
    expect(html).toContain("这次操作未能完成");
    expect(html).not.toContain("secret-value");
  });
  it("does not claim overall success from tool exit status", () => {
    const action: ToolActionSummary = { id: "a", taskId: "task-1", turnId: "turn-1", kind: "run_command",
      title: "raw command", detail: "echo raw", result: "stack output", diff: null, status: "failed", canUndo: false, createdAt: "" };
    const running = render(entries, [turn("running")], "turn-1", [action]);
    expect(running).toContain("正在处理任务");
    expect(running).not.toContain("stack output");
    expect(running).not.toContain("未能完成");
    const completed = render(entries, [turn("completed")], undefined, [action]);
    expect(completed).toContain("有操作结果需要检查");
    expect(completed).toContain("后续尝试可能已解决问题");
    expect(completed).toContain('role="alert"');
    expect(completed).toContain("final-answer");
    expect(completed).not.toContain("这次任务未能完成");
    expect(completed).not.toContain("全部成功");
    expect(completed).not.toContain("raw command");
  });
  it.each(["file_read", "command", "file_change", "error"] as const)("retains %s failure evidence outside the visible message page", (kind) => {
    const messages = [{...entry("PRIVATE_DIAGNOSTIC", kind), status: "failed" as const},
      ...Array.from({length: 205}, (_, index) => ({...entry(`hidden-${index}`), phase: "commentary" as const})),
      entry("final-answer")];
    const html = render(messages, [turn("completed")]);
    expect(html).toContain("显示更早的");
    expect(html).toContain("有操作结果需要检查");
    expect(html).not.toContain("PRIVATE_DIAGNOSTIC");
    expect(html).toContain("final-answer");
  });
  it.each(["completed", "failed", "cancelled"] as const)("does not let stale actions reactivate a %s turn", (status) => {
    const actions: ToolActionSummary[] = ["pending", "running"].map((actionStatus, index) => ({
      id:`a-${index}`,taskId:"task-1",turnId:"turn-1",kind:"run_command",status:actionStatus as "pending" | "running",
      title:"PRIVATE_COMMAND",detail:"PRIVATE_ARGUMENTS",result:null,diff:null,canUndo:false,createdAt:"",
    }));
    const html = render(entries, [turn(status)], "turn-1", actions);
    expect(html).toContain('data-running="false"');
    expect(html).toContain("部分操作尚无最终结果记录");
    expect(html).not.toContain("正在处理任务");
    expect(html).not.toContain("有操作需要你确认");
    expect(html).not.toContain("运行命令");
    expect(html).not.toContain("PRIVATE_ARGUMENTS");
    expect(html).toContain(status === "completed" ? "有操作结果需要检查" : status === "failed" ? "这次任务未能完成" : "任务已停止");
  });
  it("does not infer success from a subsequent successful action or failure from a recovered attempt", () => {
    const action: ToolActionSummary = {id:"a",taskId:"task-1",turnId:"turn-1",kind:"run_command",status:"failed",title:"raw",detail:"raw",diff:null,result:null,canUndo:false,createdAt:""};
    const html = render(entries, [turn("completed")], undefined, [action, {...action,id:"retry",status:"applied"}]);
    expect(html).toContain("有操作结果需要检查");
    expect(html).not.toContain("这次任务未能完成");
    expect(html).toContain("final-answer");
  });
  it("does not guarantee rejected operations had no effects", () => {
    const action: ToolActionSummary = {id:"a",taskId:"task-1",turnId:"turn-1",kind:"run_command",status:"rejected",title:"raw",detail:"raw",diff:null,result:null,canUndo:false,createdAt:""};
    const html = render(entries, [turn("completed")], undefined, [action]);
    expect(html).toContain("有操作未获批准，请核对实际修改");
    expect(html).not.toContain("未执行你拒绝的操作");
  });
  it("keeps unrelated turn errors from contaminating a successful turn", () => {
    const html = render(entries, [turn("completed")], undefined, [{id:"other",taskId:"task-1",turnId:"other-turn",kind:"run_command",status:"failed",title:"raw",detail:"raw",diff:null,result:null,canUndo:false,createdAt:""}]);
    expect(html).not.toContain("处理已结束");
    expect(html).not.toContain("有操作结果需要检查");
    expect(html).not.toContain('data-turn-id="turn-1"');
  });
  it("keeps approval contents and controls while suppressing execution output", () => {
    const action: ToolActionSummary = { id: "a", taskId: "task-1", turnId: "turn-1", kind: "run_command",
      title: "requested-command", detail: "requested arguments", result: "raw-result", diff: null, status: "pending",
      canUndo: false, createdAt: "", risk: "requested-risk", workingDirectory: "project-folder" };
    const html = render(entries, [turn("running")], "turn-1", [action]);
    expect(html).toContain("查看操作内容");
    expect(html).toContain("requested arguments");
    expect(html).toContain("requested-risk");
    expect(html).toContain("project-folder");
    expect(html).toContain("拒绝");
    expect(html).toContain("运行命令");
    expect(html).not.toContain("raw-result");
  });
});
