import { useEffect, useState, type FormEvent } from "react";
import type { AgentCapabilities, DesktopBridge, SaveLocalMcpServerInput } from "../bridge/types";
import { toMessage } from "../app/feedback";
import { useDialog } from "../components/useDialog";
import { ProjectMemoryPanel } from "./ProjectMemoryPanel";

export function AgentSettings({
  bridge,
  taskId,
  onClose,
}: {
  bridge: DesktopBridge;
  taskId: string;
  onClose: () => void;
}) {
  const [capabilities, setCapabilities] = useState<AgentCapabilities>();
  const dialogRef = useDialog(onClose);
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [memoryRevision, setMemoryRevision] = useState(0);
  const [forgotten, setForgotten] = useState<{ taskId: string; cutoff: number }>();
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [cwd, setCwd] = useState("");
  const [environmentVariables, setEnvironmentVariables] = useState("");

  const reload = async () => {
    setBusy(true);
    setError(undefined);
    try {
      setCapabilities(await bridge.loadAgentCapabilities(taskId));
    } catch (cause) {
      setError(toMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void reload();
  }, [taskId]);

  const saveServer = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const input: SaveLocalMcpServerInput = {
      taskId,
      name: name.trim(),
      command: command.trim(),
      args: args.split("\n").map((value) => value.trim()).filter(Boolean),
      cwd: cwd.trim() || undefined,
      environmentVariables: environmentVariables
        .split(/[\s,]+/)
        .map((value) => value.trim())
        .filter(Boolean),
      enabled: true,
    };
    setBusy(true);
    setError(undefined);
    try {
      await bridge.saveLocalMcpServer(input);
      setName("");
      setCommand("");
      setArgs("");
      setCwd("");
      setEnvironmentVariables("");
      setCapabilities(await bridge.loadAgentCapabilities(taskId));
    } catch (cause) {
      setError(toMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        ref={dialogRef}
        tabIndex={-1}
        className="model-modal agent-settings-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="agent-settings-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="create-heading">
          <div>
            <p className="eyebrow">本地 Agent 内核</p>
            <h2 id="agent-settings-title">能力与扩展</h2>
          </div>
          <button className="text-button" type="button" onClick={onClose}>关闭</button>
        </div>
        {error ? <div className="model-budget-error" role="alert">{error}</div> : null}
        {!capabilities ? <p>{busy ? "正在读取内核能力…" : "暂时无法读取能力"}</p> : (
          <>
            {capabilities.kernelNotice ? <p role="status">{capabilities.kernelNotice}</p> : null}
            <section className="agent-capability-card">
              <strong>Code Mode</strong>
              <span>{capabilities.codeModeAvailable ? "本地 host 已安装" : "缺少本地 Code Mode host"}</span>
            </section>
            <section className="agent-capability-card">
              <div>
                <strong>长期记忆</strong>
                <small>{capabilities.memoryUnavailableReason ?? "记忆按项目保存在本地，旧会话可能使用独立的历史记忆区。关闭后停止当前对话后续的自动记忆载入、专用记忆工具和记忆生成，也不再向新请求追加用户确认的记忆；不删除已有内容，也不改变文件访问权限。"}</small>
              </div>
              {!capabilities.memoryUnavailableReason ? <div className="agent-capability-actions">
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void (async () => {
                    setBusy(true);
                    setError(undefined);
                    try {
                      await bridge.setTaskMemoryEnabled(taskId, !capabilities.memoryEnabled);
                      setCapabilities({ ...capabilities, memoryEnabled: !capabilities.memoryEnabled });
                    } catch (cause) {
                      setError(toMessage(cause));
                    } finally {
                      setBusy(false);
                    }
                  })()}
                >
                  {capabilities.memoryEnabled ? "已开启" : "已关闭"}
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void (async () => {
                    if (!window.confirm("这会清空当前会话使用的项目记忆区中已自动生成的内容，且无法撤销；不会删除对话、用户确认的项目记忆或其他项目的记忆。继续使用时可能重新生成。是否继续？")) return;
                    setBusy(true);
                    setError(undefined);
                    try {
                      await bridge.resetLocalMemory(taskId);
                      setMemoryRevision((value) => value + 1);
                    } catch (cause) {
                      setError(toMessage(cause));
                    } finally {
                      setBusy(false);
                    }
                  })()}
                >
                  清空自动记忆
                </button>
                <button type="button" disabled={busy} onClick={() => void (async () => {
                  if (!window.confirm("清空当前自动记忆区，并停止从此前创建的项目对话及其继承分支自动学习。不会删除聊天记录、用户确认的项目记忆或移除当前对话已经载入的内容，其他旧记忆区不会一并清空。此操作无法撤销；后续请新建干净的任务继续学习。是否继续？")) return;
                  setBusy(true);
                  setError(undefined);
                  try {
                    const cutoff = await bridge.forgetProjectMemory(taskId);
                    setForgotten({ taskId, cutoff });
                    setMemoryRevision((value) => value + 1);
                  } catch (cause) {
                    setError(toMessage(cause));
                  } finally {
                    setBusy(false);
                  }
                })()}>清空并停止旧对话学习</button>
              </div> : <span>暂不支持</span>}
            </section>
            {forgotten?.taskId === taskId ? <p role="status">自动记忆已清空，此记忆区已停止从 {new Date(forgotten.cutoff * 1000).toLocaleString("zh-CN")} 及以前创建的对话自动学习。原聊天记录仍然保留；用户确认的记忆与已有上下文未被移除。{forgotten.cutoff * 1000 > Date.now() + 1000 ? "检测到来源时间晚于本机时间，新任务的自动学习可能暂不可用，请检查系统时间。" : ""}</p> : null}
            <section className="agent-feature-section">
              <h3>项目 Skills <span>{capabilities.skills.filter((skill) => skill.enabled).length}</span></h3>
              {capabilities.skills.length === 0 ? <small>当前项目没有可用 Skill。</small> : (
                <ul>
                  {capabilities.skills.map((skill) => (
                    <li key={`${skill.scope}:${skill.name}`}>
                      <strong>{skill.name}</strong>
                      <span>{skill.description}</span>
                    </li>
                  ))}
                </ul>
              )}
            </section>
            <section className="agent-feature-section">
              <h3>本地 MCP <span>{capabilities.mcpServers.length}</span></h3>
              {capabilities.mcpServers.map((server) => (
                <div className="mcp-server-row" key={server.name}>
                  <span><strong>{server.name}</strong><small>{server.status} · {server.toolCount} 个工具</small></span>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void (async () => {
                      setBusy(true);
                      try {
                        await bridge.removeLocalMcpServer(taskId, server.name);
                        await reload();
                      } catch (cause) {
                        setError(toMessage(cause));
                        setBusy(false);
                      }
                    })()}
                  >删除</button>
                </div>
              ))}
              <form className="mcp-server-form" onSubmit={(event) => void saveServer(event)}>
                <label><span>名称</span><input value={name} onChange={(event) => setName(event.target.value)} pattern="[A-Za-z0-9_-]{1,64}" required /></label>
                <label><span>启动程序</span><input value={command} onChange={(event) => setCommand(event.target.value)} placeholder="例如 npx 或本地 exe 路径" required /></label>
                <label><span>参数（每行一个）</span><textarea value={args} onChange={(event) => setArgs(event.target.value)} rows={3} /></label>
                <label><span>工作目录（可选，必须是绝对路径）</span><input value={cwd} onChange={(event) => setCwd(event.target.value)} /></label>
                <label><span>继承的环境变量名（可选）</span><input value={environmentVariables} onChange={(event) => setEnvironmentVariables(event.target.value)} placeholder="例如 MCP_TOKEN" /></label>
                <small>这里只保存变量名，不把密钥值写入 Simple 或 Codex 配置。</small>
                <button className="primary-action" type="submit" disabled={busy}>保存本地 MCP</button>
              </form>
            </section>
          </>
        )}
        {capabilities && !capabilities.memoryUnavailableReason ? <ProjectMemoryPanel bridge={bridge} taskId={taskId} revision={memoryRevision} /> : null}
      </section>
    </div>
  );
}
