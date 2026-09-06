import { useEffect, useRef, useState, type FormEvent } from "react";
import type {
  DesktopBridge,
  InspectorFile,
  GitWorkspaceDiff,
  TerminalCommandResult,
  TerminalSession,
} from "../bridge/types";

export type InspectorMode = "files" | "diff";

export function InspectorPanel({
  bridge,
  taskId,
  mode,
  onModeChange,
  onClose,
}: {
  bridge: DesktopBridge;
  taskId: string;
  mode: InspectorMode;
  onModeChange: (mode: InspectorMode) => void;
  onClose: () => void;
}) {
  const [review, setReview] = useState<GitWorkspaceDiff>();
  const [file, setFile] = useState<InspectorFile>();
  const [error, setError] = useState<string>();
  const reviewVersion = useRef(0);
  const fileVersion = useRef(0);
  const selectedPath = useRef<string | undefined>(undefined);
  const readFile = (path: string) => {
    selectedPath.current = path;
    const version = ++fileVersion.current;
    setError(undefined);
    setFile(undefined);
    void bridge.readProjectFile(taskId, path).then((next) => {
      if (version === fileVersion.current) setFile(next);
    }).catch((cause: unknown) => {
      if (version === fileVersion.current) setError(cause instanceof Error ? cause.message : String(cause));
    });
  };
  const refresh = () => {
    const version = ++reviewVersion.current;
    const path = selectedPath.current;
    fileVersion.current++;
    setFile(undefined);
    setError(undefined);
    void bridge
      .loadWorkspaceDiff(taskId)
      .then((next) => {
        if (version !== reviewVersion.current) return;
        setReview(next);
        // A selection made while the list loads owns its own file request.
        if (selectedPath.current !== path) return;
        if (path && next.supported && next.files.some((entry) => entry.path === path)) readFile(path);
        else selectedPath.current = undefined;
      })
      .catch((cause: unknown) => { if (version === reviewVersion.current) setError(cause instanceof Error ? cause.message : String(cause)); });
  };

  useEffect(() => {
    setReview(undefined);
    setFile(undefined);
    selectedPath.current = undefined;
    refresh();
    return () => { reviewVersion.current++; fileVersion.current++; };
  }, [bridge, mode, taskId]);

  return (
    <aside className="inspector-panel" aria-label="工作区检查器">
      <header>
        <nav aria-label="检查器视图">
          {(["files", "diff"] as const).map((value) => (
            <button
              key={value}
              type="button"
              className={mode === value ? "is-active" : ""}
              aria-pressed={mode === value}
              onClick={() => onModeChange(value)}
            >
              {value === "files" ? "文件" : "Diff"}
            </button>
          ))}
        </nav>
        <div><button type="button" aria-label="刷新检查器" title="刷新" onClick={refresh}>↻</button><button type="button" aria-label="关闭检查器" onClick={onClose}>×</button></div>
      </header>
      <div className="inspector-body">
      {error ? <p className="panel-error" role="alert">{error}</p> : null}
      {!review ? error ? null : <p className="panel-empty">正在读取本地工作区…</p> : !review.supported ? (
        <div className="panel-empty">
          <p>{review.summary}</p>
        </div>
      ) : mode === "files" ? (
        <div className="review-file-layout">
          <ul className="review-files">
            {review.files.map((entry) => (
              <li key={entry.path}>
                <button
                  type="button"
                  className={file?.path === entry.path ? "is-selected" : undefined}
                  title={entry.path}
                  onClick={() => readFile(entry.path)}
                >
                  <span>{entry.path}</span>
                  <small>+{entry.additions} −{entry.deletions}</small>
                </button>
              </li>
            ))}
          </ul>
          {file ? <pre className="file-preview" aria-label={file.path}>{file.content}{file.truncated ? "\n\n[文件过大，仅展示截断内容]" : ""}</pre> : (
            <p className="panel-empty">选择变更文件进行只读预览</p>
          )}
        </div>
      ) : (
        <pre className="inspector-diff" aria-label="代码差异">{review.unifiedDiff ? review.unifiedDiff.split("\n").map((line, index) => <span key={index} className={`diff-line ${line.startsWith("@@") ? "diff-hunk" : line.startsWith("+") && !line.startsWith("+++") ? "diff-added" : line.startsWith("-") && !line.startsWith("---") ? "diff-removed" : ""}`}>{line || " "}</span>) : "工作区没有可显示的统一 Diff"}</pre>
      )}
      </div>
    </aside>
  );
}

function splitCommandLine(value: string): string[] {
  const parts: string[] = [];
  let current = "";
  let quote: '"' | "'" | undefined;
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    if (quote) {
      if (character === quote) quote = undefined;
      else current += character;
    } else if (character === '"' || character === "'") quote = character;
    else if (/\s/.test(character)) {
      if (current) parts.push(current);
      current = "";
    } else current += character;
  }
  if (current) parts.push(current);
  return parts;
}

export function TerminalPanel({
  bridge,
  taskId,
  onClose,
}: {
  bridge: DesktopBridge;
  taskId: string;
  onClose: () => void;
}) {
  const [session, setSession] = useState<TerminalSession>();
  const [command, setCommand] = useState("");
  const [running, setRunning] = useState(false);
  const [history, setHistory] = useState<Array<{ command: string; result: TerminalCommandResult }>>([]);
  const [error, setError] = useState<string>();

  useEffect(() => {
    void bridge.openTerminal(taskId).then(setSession).catch((cause: unknown) =>
      setError(cause instanceof Error ? cause.message : String(cause)),
    );
  }, [bridge, taskId]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const parts = splitCommandLine(command.trim());
    if (!session || parts.length === 0 || running) return;
    const display = command.trim();
    setRunning(true);
    setError(undefined);
    void bridge.runTerminalCommand(session.id, parts[0], parts.slice(1)).then((result) => {
      setHistory((current) => [...current, { command: display, result }]);
      setCommand("");
    }).catch((cause: unknown) => setError(cause instanceof Error ? cause.message : String(cause)))
      .finally(() => setRunning(false));
  };

  return (
    <section className="terminal-panel" aria-label="集成终端">
      <header>
        <div><strong>终端</strong><small>{session?.cwd ?? "正在创建本地会话…"}</small></div>
        <span>{session?.processBoundary === "windows_job_object" ? "Windows Job Object 进程生命周期管理" : "仅管理直接进程"}</span>
        <button type="button" aria-label="收起终端" onClick={onClose}>⌄</button>
      </header>
      <div className="terminal-output" aria-live="polite">
        {history.map(({ command: display, result }) => (
          <div key={result.commandId}>
            <strong>&gt; {display}</strong>
            {result.stdout ? <pre>{result.stdout}</pre> : null}
            {result.stderr ? <pre className="terminal-stderr">{result.stderr}</pre> : null}
            <small>退出码：{result.exitCode ?? "未启动"}{result.truncated ? " · 输出已截断" : ""}</small>
          </div>
        ))}
        {error ? <p className="panel-error" role="alert">{error}</p> : null}
      </div>
      <form onSubmit={submit}>
        <span aria-hidden="true">›</span>
        <input value={command} disabled={!session || running} onChange={(event) => setCommand(event.target.value)} aria-label="终端命令" placeholder="输入程序和参数（直接执行，不经 shell 拼接）" />
        <button type="submit" disabled={!session || running || !command.trim()}>{running ? "运行中" : "运行"}</button>
      </form>
    </section>
  );
}
