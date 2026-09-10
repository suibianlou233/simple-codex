import { useEffect, useRef, useState } from "react";
import type {
  DesktopBridge,
  InspectorFile,
  GitWorkspaceDiff,
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

export { TerminalPanel } from "./TerminalPanel";
