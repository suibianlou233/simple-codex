import { Fragment, useEffect, useState } from "react";

import type { TimelineEntry, ToolActionSummary, TurnSummary } from "../bridge/types";
import { ItemRendererView } from "./ItemRendererRegistry";
import { CopyButton } from "../components/CopyButton";
import { ElapsedTime, elapsedBetween, formatElapsed } from "../components/ElapsedTime";
import { toMessage } from "../app/feedback";
import { SubagentReport } from "./SubagentReport";

export function TaskTimeline({
  entries,
  actions,
  activeTurnId,
  turns = [],
  disabled,
  onApprove,
  onReject,
  onUndo,
  onCancel,
  onRevise,
  onRegenerate,
  onBranch,
}: {
  entries: TimelineEntry[];
  actions: ToolActionSummary[];
  activeTurnId?: string | null;
  turns?: TurnSummary[];
  disabled: boolean;
  onApprove: (actionId: string) => Promise<void>;
  onReject: (actionId: string) => Promise<void>;
  onUndo: (actionId: string) => Promise<void>;
  onCancel: (actionId: string) => Promise<boolean>;
  onRevise: (messageId: string, content: string) => Promise<boolean>;
  onRegenerate: () => Promise<void>;
  onBranch: (messageId: string) => Promise<void>;
}) {
  const [editingMessageId, setEditingMessageId] = useState<string>();
  const [editDraft, setEditDraft] = useState("");
  const [editError, setEditError] = useState<string>();
  const [editSubmitting, setEditSubmitting] = useState(false);
  const [visibleLimit, setVisibleLimit] = useState(200);
  useEffect(() => setVisibleLimit(200), [entries.at(-1)?.taskId]);
  const visibleEntries = entries.slice(Math.max(0, entries.length - visibleLimit));
  const lastAssistantId = [...entries]
    .reverse()
    .find((entry) => entry.kind === "assistant" && entry.phase !== "commentary")?.id;
  const actionsByTurn = new Map<string, ToolActionSummary[]>();
  for (const action of actions) {
    const turnActions = actionsByTurn.get(action.turnId) ?? [];
    turnActions.push(action);
    actionsByTurn.set(action.turnId, turnActions);
  }
  const readEvidenceByTurn = new Map<string, TimelineEntry[]>();
  // Rendering is paginated; outcome evidence must not disappear with older rows.
  for (const entry of entries) {
    if (entry.turnId && ["file_read", "command", "file_change", "error"].includes(entry.kind)) {
      const evidence = readEvidenceByTurn.get(entry.turnId) ?? [];
      evidence.push(entry);
      readEvidenceByTurn.set(entry.turnId, evidence);
    }
  }
  const renderedProcessTurnIds = new Set<string>();
  const turnsById = new Map(turns.map((turn) => [turn.id, turn]));
  // A prior turn's cached canUndo must not enable writes during a newer turn.
  // The backend also checks project-wide ownership, including other tasks/apps.
  const undoBlocked = Boolean(activeTurnId && !turnsById.has(activeTurnId)) ||
    turns.some((turn) => turn.status === "running");
  const lastAssistantByTurn = new Map<string, string>();
  for (const entry of entries) {
    if (entry.turnId && entry.kind === "assistant" && entry.phase !== "commentary") lastAssistantByTurn.set(entry.turnId, entry.id);
  }
  const foldableCommentary = (entry: TimelineEntry) => Boolean(
    entry.kind === "assistant"
    && entry.phase === "commentary"
    && entry.turnId
    && turnsById.has(entry.turnId)
    && turnsById.get(entry.turnId)?.status !== "running",
  );
  // Native final answers can stream; commentary stays in the local journal.
  // Old/provider messages without phase keep the conservative completion fallback.
  const showAssistant = (entry: TimelineEntry) => {
    if (!entry.turnId) return entry.phase !== "commentary";
    const turn = turnsById.get(entry.turnId);
    if (entry.phase === "commentary") {
      return entry.turnId === activeTurnId || Boolean(turn && turn.status !== "completed");
    }
    if (turn?.status === "failed" || turn?.status === "cancelled") return false;
    if (entry.phase === "final_answer") return true;
    if (turn?.status === "running" || (!turn && entry.turnId === activeTurnId)) return false;
    return entry.id === lastAssistantByTurn.get(entry.turnId);
  };
  const canRenderEntry = (entry: TimelineEntry) => {
    if (["lifecycle", "file_read", "command", "file_change"].includes(entry.kind)) return false;
    if (entry.kind === "assistant" && !showAssistant(entry) && !foldableCommentary(entry)) return false;
    return !(entry.kind === "error" && entry.turnId &&
      (turnsById.has(entry.turnId) || entry.turnId === activeTurnId));
  };
  const groups: TimelineEntry[][] = [];
  for (const entry of visibleEntries.filter(canRenderEntry)) {
    const previous = groups.at(-1);
    if (foldableCommentary(entry) && previous && foldableCommentary(previous[0])
      && previous[0].turnId === entry.turnId) previous.push(entry);
    else groups.push([entry]);
  }
  // Anchor each status to a row that actually renders, never to hidden tool/error
  // records. A cancelled text-only turn still owns its user-message position.
  const lastVisibleEntryIdByTurn = new Map<string, string>();
  for (const entry of visibleEntries) {
    if (entry.turnId && canRenderEntry(entry)) {
      lastVisibleEntryIdByTurn.set(entry.turnId, entry.id);
    }
  }
  const renderProcess = (entry: TimelineEntry) => {
    const turnId = entry.turnId;
    if (!turnId || lastVisibleEntryIdByTurn.get(turnId) !== entry.id || renderedProcessTurnIds.has(turnId)) return null;
    renderedProcessTurnIds.add(turnId);
    const turn = turnsById.get(turnId);
    return <WorkProcess turnId={turnId} actions={actionsByTurn.get(turnId) ?? []}
      evidence={readEvidenceByTurn.get(turnId) ?? []} phase={turn?.phase}
      startedAt={turn?.startedAt} outcome={turn?.status} childReport={turn?.childReport}
      failureMessage={entries.find((item) => item.turnId === turnId && item.kind === "error")?.detail}
      active={activeTurnId === turnId} undoBlocked={undoBlocked} disabled={disabled}
      onApprove={onApprove} onReject={onReject} onUndo={onUndo} onCancel={onCancel} />;
  };
  const renderCompletedElapsed = (entry: TimelineEntry) => {
    const turn = entry.turnId ? turnsById.get(entry.turnId) : undefined;
    if (!turn || turn.status === "running") return null;
    const elapsed = elapsedBetween(turn.startedAt, turn.finishedAt);
    return elapsed === null ? null : <span className="turn-elapsed"> · 本轮用时 {formatElapsed(elapsed)}</span>;
  };
  const renderEntry = (entry: TimelineEntry) => {
        if (!canRenderEntry(entry)) return null;
        return (
          <Fragment key={entry.id}>
            <article className={`chat-message chat-${entry.kind}`}>
              <div className="message-column">
                {editingMessageId === entry.id ? (
                  <form
                    className="message-editor"
                    onSubmit={(event) => {
                      event.preventDefault();
                      const content = editDraft.trim();
                      if (!content || disabled || editSubmitting || activeTurnId) return;
                      if (!window.confirm("重新发送会回退这条消息之后的对话，但不会撤销已经写入工作区的文件。是否继续？")) return;
                      setEditError(undefined);
                      setEditSubmitting(true);
                      void onRevise(entry.id, content).then((sent) => {
                        if (sent) setEditingMessageId(undefined);
                        else setEditError("未能重新发送，编辑内容已保留，请稍后重试。");
                      }).catch(() => setEditError("未能重新发送，编辑内容已保留，请稍后重试。"))
                        .finally(() => setEditSubmitting(false));
                    }}
                  >
                    <textarea aria-label="编辑消息内容" value={editDraft} disabled={editSubmitting} onChange={(event) => setEditDraft(event.target.value)} />
                    {editError ? <p role="alert">{editError}</p> : null}
                    <div>
                      <button type="button" disabled={editSubmitting} onClick={() => setEditingMessageId(undefined)}>取消</button>
                      <button type="submit" disabled={disabled || editSubmitting || Boolean(activeTurnId) || !editDraft.trim()}>重新发送</button>
                    </div>
                  </form>
                ) : (
                  <ItemRendererView item={entry} />
                )}
                {editingMessageId !== entry.id ? (
                  <div className="message-actions">
                    {entry.kind === "user" || entry.kind === "assistant" ? <CopyButton text={entry.detail ?? entry.title} /> : null}
                    {entry.kind === "user" ? (
                      <button
                        type="button"
                        disabled={disabled || Boolean(activeTurnId)}
                        onClick={() => {
                          setEditingMessageId(entry.id);
                          setEditError(undefined);
                          setEditDraft(entry.detail ?? entry.title);
                        }}
                      >
                        编辑
                      </button>
                    ) : null}
                    {entry.kind === "assistant" && entry.id === lastAssistantId ? (
                      <button
                        type="button"
                        disabled={disabled || Boolean(activeTurnId)}
                        title="只回退对话，不会撤销已经写入工作区的文件"
                        onClick={() => {
                          if (window.confirm("重新生成会回退上一轮对话，但不会撤销已经写入工作区的文件。是否继续？")) {
                            void onRegenerate();
                          }
                        }}
                      >
                        重新生成
                      </button>
                    ) : null}
                    {entry.kind === "user" || entry.kind === "assistant" ? (
                      <button type="button" disabled={disabled || Boolean(activeTurnId)} onClick={() => void onBranch(entry.id)}>
                        从这里分支
                      </button>
                    ) : null}
                  </div>
                ) : null}
              </div>
            </article>
            {!foldableCommentary(entry) ? renderProcess(entry) : null}
          </Fragment>
        );
  };
  return (
    <div className="timeline-stack">
      {visibleEntries.length < entries.length ? (
        <button className="load-earlier" type="button" onClick={() => setVisibleLimit((value) => value + 200)}>
          显示更早的 {Math.min(200, entries.length - visibleEntries.length)} 条记录
        </button>
      ) : null}
      {groups.map((group) => foldableCommentary(group[0]) ? (
        <Fragment key={`${group[0].id}-process`}>
        <details className="completed-commentary">
          <summary>执行过程 · {group.length} 条说明{renderCompletedElapsed(group[0])}</summary>
          <div className="completed-commentary-content">{group.map(renderEntry)}</div>
        </details>
        {renderProcess(group[group.length - 1])}
        </Fragment>
      ) : renderEntry(group[0]))}
      {[...new Set([
        ...turns.filter((turn) => entries.some((entry) => entry.taskId === turn.taskId)).map((turn) => turn.id),
        ...actionsByTurn.keys(),
        ...readEvidenceByTurn.keys(),
        ...turns.filter((turn) => turn.childReport).map((turn) => turn.id),
        ...(activeTurnId ? [activeTurnId] : []),
      ])]
        .filter((turnId) => !renderedProcessTurnIds.has(turnId))
        // Live work and an action-only initial snapshot may need a footer before
        // messages arrive. Never reattach off-page history below the latest reply.
        .filter((turnId) => turnsById.get(turnId)?.status === "running" ||
            (!turnsById.has(turnId) && ((entries.length === 0 && turns.length === 0) || activeTurnId === turnId ||
            actionsByTurn.get(turnId)?.some((action) => action.status === "pending" || action.status === "running"))))
        .map((turnId) => (
          <WorkProcess
            key={turnId}
            turnId={turnId}
            actions={actionsByTurn.get(turnId) ?? []}
            evidence={readEvidenceByTurn.get(turnId) ?? []}
            phase={turnsById.get(turnId)?.phase}
            startedAt={turnsById.get(turnId)?.startedAt}
            outcome={turnsById.get(turnId)?.status}
            childReport={turnsById.get(turnId)?.childReport}
            failureMessage={entries.find((item) => item.turnId === turnId && item.kind === "error")?.detail}
            active={activeTurnId === turnId}
            undoBlocked={undoBlocked}
            disabled={disabled}
            onApprove={onApprove}
            onReject={onReject}
            onUndo={onUndo}
            onCancel={onCancel}
          />
        ))}
    </div>
  );
}

const actionStatusLabels: Record<ToolActionSummary["status"], string> = {
  pending: "等待确认",
  running: "运行中",
  applied: "已完成",
  rejected: "已拒绝",
  failed: "失败",
  undone: "已撤销",
};

export function summarizeWorkProcess(
  actions: ToolActionSummary[],
  active: boolean,
  evidence: TimelineEntry[] = [],
): string {
  return workProcessState(actions, active, evidence).summary;
}

function workProcessState(
  actions: ToolActionSummary[], active: boolean, evidence: TimelineEntry[],
  outcome?: TurnSummary["status"], childReport?: TurnSummary["childReport"], phase?: TurnSummary["phase"],
) {
  const terminal = outcome !== undefined && outcome !== "running";
  const uncertain = !terminal && (phase === "checking_submission" || phase === "submission_recovery_required");
  const pending = terminal || uncertain ? [] : actions.filter((action) => action.status === "pending");
  const running = !terminal && !uncertain && (active || outcome === "running" || actions.some((action) => action.status === "running"));
  const failure = outcome === "failed";
  const failedOperations = actions.some((action) => action.status === "failed") ||
    evidence.some((entry) => entry.status === "failed" || entry.kind === "error");
  const rejectedOperations = actions.some((action) => action.status === "rejected");
  const unsettledOperations = terminal && actions.some((action) => action.status === "running" || action.status === "pending");
  const operationIssues = failedOperations || rejectedOperations || unsettledOperations;
  const childIssues = Boolean(childReport && (childReport.rejectedOperations > 0 || childReport.outcomes.some((child) => child.status !== "completed") || childReport.assignments?.some((item) => item.status !== "dispatched" || item.receivers.some((id) => !childReport.outcomes.some((child) => child.threadId === id)))));
  const needsReview = uncertain || (!running && !pending.length && (operationIssues || childIssues));
  const summary = failure ? "这次任务未能完成" : outcome === "cancelled" ? "任务已停止"
    : uncertain ? "任务执行状态待确认，请勿重复发送" : pending.length ? "有操作需要你确认" : running ? (phase === "preparing_kernel" ? "正在连接执行内核，可停止…" : "正在处理任务…")
    : operationIssues ? "处理已结束，有操作结果需要检查"
    : childIssues ? "处理已结束，有子任务结果需要检查"
    : actions.some((action) => action.status === "undone") ? "已撤销相关修改" : "处理已结束";
  return { pending, running, failure, failedOperations, rejectedOperations, unsettledOperations, childIssues, needsReview, summary, uncertain };
}

function WorkProcess({
  turnId, actions, evidence, active, outcome, childReport, phase, startedAt, failureMessage, disabled, undoBlocked,
  onApprove, onReject, onUndo, onCancel,
}: {
  turnId: string;
  actions: ToolActionSummary[];
  evidence: TimelineEntry[];
  active: boolean;
  outcome?: TurnSummary["status"];
  childReport?: TurnSummary["childReport"];
  phase?: TurnSummary["phase"];
  startedAt?: string | null;
  failureMessage?: string;
  disabled: boolean;
  undoBlocked: boolean;
  onApprove: (actionId: string) => Promise<void>;
  onReject: (actionId: string) => Promise<void>;
  onUndo: (actionId: string) => Promise<void>;
  onCancel: (actionId: string) => Promise<boolean>;
}) {
  const { pending, running, failure, failedOperations, rejectedOperations, unsettledOperations, childIssues, needsReview, summary, uncertain } =
    workProcessState(actions, active, evidence, outcome, childReport, phase);
  const undoableActions = actions.filter((action) => action.canUndo && !undoBlocked);
  // Submission protection stays in the runtime/composer; omit this duplicate row.
  const showSummary = !uncertain && (running || pending.length > 0 || failure || needsReview
    || outcome === "cancelled" || actions.some((action) => action.status === "undone"));
  if (!showSummary && !childReport && undoableActions.length === 0) return null;
  return (
    <section className="work-process-wrap" data-turn-id={turnId}>
      {showSummary ? (
        <div className="work-process work-process-live" data-running={running} data-attention={failure || needsReview} role={failure || needsReview ? "alert" : "status"}>
          <span className="work-process-dot" aria-hidden="true" />
          <strong>{summary}</strong>
          {running && pending.length === 0 ? <ElapsedTime startedAt={startedAt} /> : null}
        </div>
      ) : null}
      {failure ? <p className="user-task-notice">{toMessage(failureMessage)} 已完成的修改可能仍保留，请先检查当前项目。</p> : null}
      {outcome === "cancelled" ? <p className="user-task-notice">停止不会自动撤销已经完成的修改。</p> : null}
      {needsReview && failedOperations ? <p className="user-task-notice">执行过程中有操作未成功。后续尝试可能已解决问题，请结合最终回复检查修改并验证结果。</p> : null}
      {needsReview && rejectedOperations ? <p className="user-task-notice">有操作未获批准，请核对实际修改。</p> : null}
      {needsReview && unsettledOperations ? <p className="user-task-notice">本轮已结束，但部分操作尚无最终结果记录。请刷新状态并检查项目；这不表示操作仍在运行或未产生修改。</p> : null}
      {!running && childIssues ? <p className="user-task-notice">部分子任务未成功结束、结果尚未确认，或有操作被拒绝。请检查结果和项目修改。</p> : null}
      {childReport ? <SubagentReport report={childReport} ended={outcome !== "running"} /> : null}
      {pending.length ? <div className="work-process-attention">
        {pending.map((action) => <ActionCard key={action.id} action={action} disabled={disabled}
          onApprove={onApprove} onReject={onReject} onUndo={onUndo} onCancel={onCancel} />)}
      </div> : null}
      {undoableActions.map((action) => (
        <div className="action-controls" key={action.id}>
          <span>{action.title}</span>
          <button type="button" disabled={disabled} onClick={() => void onUndo(action.id)}>撤销修改</button>
        </div>
      ))}
    </section>
  );
}

export { MarkdownMessage } from "./MarkdownMessage";

function ActionCard({
  action,
  disabled,
  onApprove,
  onReject,
  onUndo,
  onCancel,
}: {
  action: ToolActionSummary;
  disabled: boolean;
  onApprove: (actionId: string) => Promise<void>;
  onReject: (actionId: string) => Promise<void>;
  onUndo: (actionId: string) => Promise<void>;
  onCancel: (actionId: string) => Promise<boolean>;
}) {
  const status = actionStatusLabels[action.status];
  const [cancelRequested, setCancelRequested] = useState(false);
  useEffect(() => {
    if (action.status !== "running") setCancelRequested(false);
  }, [action.status]);
  return (
    <article className={`action-card action-${action.status}`}>
      <div className="action-heading">
        <div>
          <span>{action.isSubagent ? "子任务需要你确认" : "需要你确认"}</span>
          <h3>{action.kind === "write_file" ? "允许修改项目文件？" : "允许执行本地操作？"}</h3>
        </div>
        <strong>{status}</strong>
      </div>
      <p className="action-detail">{action.kind === "write_file"
        ? "将修改下面列出的文件，请核对改动内容。"
        : "将运行一项本地命令，可能读取或修改文件。确认前请核对操作内容。"} </p>
      {action.risk ? <p className="action-risk">{action.risk}</p> : null}
      <details className="approval-operation">
        <summary>查看操作内容</summary>
        <p>{action.title}</p>
        <pre>{action.detail}</pre>
      {action.workingDirectory ? <p className="action-meta">工作目录：{action.workingDirectory}</p> : null}
      {action.requestedCapabilities?.length ? (
        <p className="action-meta">请求能力：{action.requestedCapabilities.join("、")}</p>
      ) : null}
      {action.diff ? <pre className="diff-preview">{action.diff}</pre> : null}
      </details>
      <div className="action-controls">
        {action.status === "pending" ? (
          <>
            <button type="button" className="reject-action" disabled={disabled} onClick={() => void onReject(action.id)}>
              拒绝
            </button>
            <button type="button" className="approve-action" disabled={disabled} onClick={() => void onApprove(action.id)}>
              {action.kind === "write_file" ? "应用修改" : "运行命令"}
            </button>
          </>
        ) : null}
        {action.status === "running" ? (
          <button
            type="button"
            className="reject-action"
            disabled={disabled || cancelRequested}
            onClick={() => {
              setCancelRequested(true);
              void onCancel(action.id).then((requested) => {
                if (!requested) setCancelRequested(false);
              });
            }}
          >
            停止任务
          </button>
        ) : null}
        {action.canUndo ? (
          <button type="button" className="reject-action" disabled={disabled} onClick={() => void onUndo(action.id)}>撤销修改</button>
        ) : null}
      </div>
    </article>
  );
}
