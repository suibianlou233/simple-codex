import { useEffect, useMemo, useRef, useState } from "react";
import { desktopBridge } from "../bridge";
import { sendConversationMessage } from "../conversation";
import { toggleInspector } from "./interactions";
import { draftKey } from "./navigation";
import { Sidebar } from "../components/Sidebar";
import { TaskSearch } from "../components/TaskSearch";
import { Icon } from "../components/Icon";
import { useDesktopProjection } from "../store/entityStore";
import { TaskTimeline } from "../items/TaskTimeline";
import { Composer } from "../components/Composer";
import { EmptyWorkspace } from "../components/Welcome";
import { AgentSettings } from "../settings/AgentSettings";
import { ModelSettings } from "../settings/ModelSettings";
import { FirstRunGuide } from "../components/FirstRunGuide";
import { markGuideSeen, shouldShowGuide } from "./onboarding";
import { toMessage } from "./feedback";
import { InspectorPanel, TerminalPanel, type InspectorMode } from "../panels/WorkspacePanels";
import {
  applyResolvedTheme,
  persistThemePreference,
  readThemePreference,
  resolveTheme,
  systemPrefersDark,
  type ThemePreference,
} from "../theme";
import type {
  AttachmentSummary,
  DesktopBridge,
  PermissionLevel,
} from "../bridge/types";

type AppProps = {
  bridge?: DesktopBridge;
};

export function permissionForNextConversation(
  alreadyComposingNewConversation: boolean,
  selectedTaskPermission: PermissionLevel | undefined,
  draftPermission: PermissionLevel,
): PermissionLevel {
  if (alreadyComposingNewConversation) return draftPermission;
  return selectedTaskPermission ?? draftPermission;
}

export function WorkspaceNavigation({
  inspectorMode,
  terminalOpen,
  onInspectorChange,
  onTerminalToggle,
}: {
  inspectorMode?: InspectorMode;
  terminalOpen: boolean;
  onInspectorChange: (mode: InspectorMode) => void;
  onTerminalToggle: () => void;
}) {
  return (
    <nav className="sidebar-workspace-nav" aria-label="工作区工具">
      {(["files", "diff"] as const).map((mode) => (
        <button
          className={inspectorMode === mode ? "is-active" : undefined}
          type="button"
          aria-pressed={inspectorMode === mode}
          aria-label={mode === "files" ? "文件" : "Diff"}
          title={mode === "files" ? "变更文件" : mode === "diff" ? "工作区差异" : "本轮改动"}
          key={mode}
          onClick={() => onInspectorChange(mode)}
        >
          <Icon name={mode === "files" ? "folder" : "diff"} size={16} /><span>{mode === "files" ? "文件" : "Diff"}</span>
        </button>
      ))}
      <button
        className={terminalOpen ? "is-active" : undefined}
        type="button"
        aria-pressed={terminalOpen}
        aria-label="终端"
        onClick={onTerminalToggle}
      >
        <Icon name="terminal" size={16} /><span>终端</span>
      </button>
    </nav>
  );
}

export function App({ bridge = desktopBridge }: AppProps) {
  const { snapshot, error: projectionError } = useDesktopProjection(bridge);
  const [isNewConversation, setIsNewConversation] = useState(false);
  const [isBusy, setIsBusy] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isGuideOpen, setIsGuideOpen] = useState(shouldShowGuide);
  const closeGuide = () => { markGuideSeen(); setIsGuideOpen(false); };
  const [isAgentSettingsOpen, setIsAgentSettingsOpen] = useState(false);
  const [sendAfterConfiguration, setSendAfterConfiguration] = useState(false);
  const [pendingConfiguredMessage, setPendingConfiguredMessage] = useState("");
  const [drafts, setDrafts] = useState<Record<string, { content: string; attachments: AttachmentSummary[] }>>({});
  const currentDraftKey = draftKey(snapshot?.activeProjectId, isNewConversation ? undefined : snapshot?.activeTaskId);
  const composerDraft = drafts[currentDraftKey]?.content ?? "";
  const attachments = drafts[currentDraftKey]?.attachments ?? [];
  const setComposerDraft = (content: string) => setDrafts((all) => ({ ...all, [currentDraftKey]: { content, attachments: all[currentDraftKey]?.attachments ?? [] } }));
  const setAttachments = (next: AttachmentSummary[] | ((current: AttachmentSummary[]) => AttachmentSummary[])) => setDrafts((all) => ({ ...all, [currentDraftKey]: { content: all[currentDraftKey]?.content ?? "", attachments: typeof next === "function" ? next(all[currentDraftKey]?.attachments ?? []) : next } }));
  const [newConversationPermission, setNewConversationPermission] =
    useState<PermissionLevel>("approval");
  const [searchOpen, setSearchOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [notice, setNotice] = useState<string>();
  const [error, setError] = useState<string>();
  const [inspectorMode, setInspectorMode] = useState<InspectorMode | undefined>(() => {
    if (typeof window === "undefined") return undefined;
    const saved = window.localStorage.getItem("simple.ui.inspector");
    return saved === "files" || saved === "diff" ? saved : saved === "review" ? "diff" : undefined;
  });
  const [terminalOpen, setTerminalOpen] = useState(false);
  const restoredSelection = useRef(false);
  const timelineRef = useRef<HTMLDivElement>(null);
  const [followOutput, setFollowOutput] = useState(true);
  const [themePreference, setThemePreference] = useState<ThemePreference>(() =>
    readThemePreference(),
  );
  const [prefersDark, setPrefersDark] = useState(() => systemPrefersDark());

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = (event: MediaQueryListEvent) => setPrefersDark(event.matches);
    setPrefersDark(media.matches);
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

  const resolvedTheme = resolveTheme(themePreference, prefersDark);

  useEffect(() => {
    applyResolvedTheme(resolvedTheme);
    persistThemePreference(themePreference);
  }, [resolvedTheme, themePreference]);

  useEffect(() => {
    if (projectionError) setError(toMessage(projectionError));
  }, [projectionError]);

  useEffect(() => {
    if (!snapshot || restoredSelection.current || typeof window === "undefined") return;
    restoredSelection.current = true;
    const savedTaskId = window.localStorage.getItem("simple.ui.activeTaskId");
    if (savedTaskId && savedTaskId !== snapshot.activeTaskId && snapshot.tasks.some((task) => task.id === savedTaskId)) {
      void bridge.selectTask(savedTaskId);
    }
  }, [bridge, snapshot]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    if (snapshot?.activeTaskId) window.localStorage.setItem("simple.ui.activeTaskId", snapshot.activeTaskId);
    if (inspectorMode) window.localStorage.setItem("simple.ui.inspector", inspectorMode);
    else window.localStorage.removeItem("simple.ui.inspector");
  }, [inspectorMode, snapshot?.activeTaskId]);

  const activeProject = snapshot?.projects.find(
    (project) => project.id === snapshot.activeProjectId,
  );
  const selectedTask = snapshot?.tasks.find(
    (task) => task.id === snapshot.activeTaskId,
  );
  const activeTask = isNewConversation ? undefined : selectedTask;
  const activeModel = snapshot?.modelProfiles.find(
    (profile) => profile.id === snapshot.activeModelProfileId,
  );
  const activeTimeline = useMemo(
    () =>
      snapshot?.timeline.filter((entry) => entry.taskId === snapshot.activeTaskId) ?? [],
    [snapshot],
  );
  const activeActions = useMemo(
    () => snapshot?.actions.filter((action) => action.taskId === snapshot.activeTaskId) ?? [],
    [snapshot],
  );
  const activeTurns = useMemo(
    () => snapshot?.turns.filter((turn) => turn.taskId === snapshot.activeTaskId) ?? [],
    [snapshot],
  );
  const activeContextUsage = snapshot?.contextUsage.find(
    (usage) => usage.taskId === activeTask?.id,
  );
  useEffect(() => {
    setFollowOutput(true);
  }, [snapshot?.activeTaskId]);
  useEffect(() => {
    if (!followOutput) return;
    const element = timelineRef.current;
    if (element) element.scrollTop = element.scrollHeight;
  }, [activeActions, activeTimeline, followOutput]);
  const run = async (operation: () => Promise<void>): Promise<boolean> => {
    setIsBusy(true);
    setError(undefined);
    setNotice(undefined);
    try {
      await operation();
      return true;
    } catch (cause: unknown) {
      setError(toMessage(cause));
      return false;
    } finally {
      setIsBusy(false);
    }
  };

  const beginNewConversation = () => {
    if (!activeProject || isBusy) return;
    setNewConversationPermission(permissionForNextConversation(isNewConversation, selectedTask?.permissionLevel, newConversationPermission));
    setIsNewConversation(true);
    requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus());
  };
  useEffect(() => {
    const handleKey = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey || isSettingsOpen || isAgentSettingsOpen || isGuideOpen) return;
      if (event.key.toLowerCase() === "k") { event.preventDefault(); setSearchOpen((open) => !open); }
      if (event.key.toLowerCase() === "b") { event.preventDefault(); setSidebarCollapsed((collapsed) => !collapsed); }
      if (event.key.toLowerCase() === "n") { event.preventDefault(); beginNewConversation(); }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  });

  if (!snapshot) {
    return (
      <main className="loading-shell">
        <p>{error ?? "正在打开本地工作区…"}</p>
      </main>
    );
  }

  return (
    <main className={`app-shell${sidebarCollapsed ? " sidebar-collapsed" : ""}${inspectorMode && activeTask ? " has-inspector" : ""}${terminalOpen && activeTask ? " has-terminal" : ""}`}>
      {!sidebarCollapsed ? <Sidebar snapshot={snapshot} activeTaskId={activeTask?.id} busy={isBusy}
        theme={resolvedTheme} onTheme={setThemePreference} onCollapse={() => setSidebarCollapsed(true)}
        onSearch={() => setSearchOpen(true)} onNew={beginNewConversation}
        onOpen={() => void run(async () => { await bridge.openProject(); setIsNewConversation(true); setNewConversationPermission("approval"); })}
        onProject={(project) => void run(async () => { await bridge.selectProject(project.id); setIsNewConversation(true); setNewConversationPermission("approval"); })}
        onTask={(task) => void run(async () => { await bridge.selectTask(task.id); setIsNewConversation(false); setNewConversationPermission(task.permissionLevel); })}
        onSettings={() => { setSendAfterConfiguration(false); setIsSettingsOpen(true); }}
        onAgent={() => setIsAgentSettingsOpen(true)}
        onExport={() => void run(async () => { if (!activeTask) return; const path = await bridge.exportConversation(activeTask.id); if (path) setNotice(`已导出到 ${path}`); })}
        onDiagnostics={() => void run(async () => { const path = await bridge.exportDiagnostics(); if (path) setNotice(`诊断报告已导出到 ${path}`); })}
      /> : null}

      <section className={`workspace${activeTask ? "" : " workspace-welcome"}`}>
        <header className="workspace-header">
          <div className="workspace-heading">
            {sidebarCollapsed ? <button className="icon-button" onClick={() => setSidebarCollapsed(false)} aria-label="展开侧栏" title="展开侧栏（Ctrl+B）"><Icon name="sidebar" /></button> : null}
            <span className={`workspace-status${activeTask ? ` status-${activeTask.status}` : ""}`} aria-hidden="true" />
            <div>
              <strong title={activeTask?.title}>{activeTask?.title ?? (activeProject ? "新对话" : "Simple")}</strong>
              <small title={activeProject?.path}>{activeProject?.name ?? "本地工作区"}<span className="breadcrumb-divider">/</span>{activeTask ? "任务" : "开始新任务"}</small>
            </div>
          </div>
          {activeTask ? (
            <WorkspaceNavigation
              inspectorMode={inspectorMode}
              terminalOpen={terminalOpen}
              onInspectorChange={(mode) => setInspectorMode((current) => toggleInspector(current, mode))}
              onTerminalToggle={() => setTerminalOpen((value) => !value)}
            />
          ) : (
            <button className="header-search" onClick={() => setSearchOpen(true)}><Icon name="search" size={16} />搜索任务</button>
          )}
        </header>

        <div className="workspace-banners">
          {error ? (
            <div className="error-banner" role="alert">
              <span>{error}</span>
              <button type="button" onClick={() => setError(undefined)}>
                关闭
              </button>
            </div>
          ) : null}
          {notice ? (
            <div className="notice-banner" role="status">
              <span>{notice}</span>
              <button type="button" onClick={() => setNotice(undefined)}>关闭</button>
            </div>
          ) : null}
        </div>

        <div
          className="timeline"
          aria-label="对话记录"
          ref={timelineRef}
          onScroll={(event) => {
            const element = event.currentTarget;
            setFollowOutput(element.scrollHeight - element.scrollTop - element.clientHeight < 72);
          }}
        >
          {activeTask ? (
            <TaskTimeline
              key={activeTask.id}
              entries={activeTimeline}
              actions={activeActions}
              activeTurnId={snapshot.activeTurnId}
              turns={activeTurns}
              disabled={isBusy}
              onApprove={async (actionId) => { await run(() => bridge.approveAction(actionId)); }}
              onReject={async (actionId) => { await run(() => bridge.rejectAction(actionId)); }}
              onUndo={async (actionId) => { await run(() => bridge.undoAction(actionId)); }}
              onCancel={(actionId) => run(() => bridge.cancelAction(actionId))}
              onRevise={async (messageId, content) => {
                if (!activeTask) return false;
                return run(() => bridge.reviseMessage(activeTask.id, messageId, content));
              }}
              onRegenerate={async () => {
                if (!activeTask) return;
                await run(() => bridge.regenerateResponse(activeTask.id));
              }}
              onBranch={async (messageId) => {
                if (!activeTask) return;
                const branched = await run(() => bridge.branchConversation(activeTask.id, messageId));
                if (branched) setIsNewConversation(false);
              }}
            />
          ) : (
            <EmptyWorkspace
              hasProject={Boolean(activeProject)}
              projectName={activeProject?.name}
              onSuggestion={(value) => { setComposerDraft(value); requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus()); }}
              onOpenProject={() => void run(async () => {
                await bridge.openProject();
                setIsNewConversation(true);
              })}
            />
          )}
        </div>
        {!followOutput && activeTask ? (
          <button
            className="resume-follow"
            type="button"
            onClick={() => {
              setFollowOutput(true);
              const element = timelineRef.current;
              if (element) element.scrollTop = element.scrollHeight;
            }}
          >
            ↓ 回到最新消息
          </button>
        ) : null}

        <Composer
          projectId={activeProject?.id}
          projectName={activeProject?.name}
          modelProfiles={snapshot.modelProfiles}
          activeModelId={snapshot.activeModelProfileId}
          onModelChange={(id) => run(() => bridge.selectModelProfile(id))}
          onModelSettings={() => { setSendAfterConfiguration(false); setIsSettingsOpen(true); }}
          contextUsage={activeContextUsage}
          phase={snapshot.turns.find((turn) => turn.id === snapshot.activeTurnId)?.phase}
          activeTurnId={snapshot.activeTurnId}
          disabled={isBusy}
          content={composerDraft}
          attachments={attachments}
          onContentChange={setComposerDraft}
          onAttach={async () => {
            if (!activeProject) return;
            await run(async () => {
            const selected = await bridge.pickAttachments(activeProject.id);
            setAttachments((current) => {
              const byPath = new Map(current.map((item) => [item.path, item]));
              for (const item of selected) byPath.set(item.path, item);
              return [...byPath.values()];
            });
            });
          }}
          onRemoveAttachment={(path) => {
            setAttachments((current) => current.filter((item) => item.path !== path));
          }}
          onSend={async (content) => {
            if (!activeProject) return;
            if (!activeModel) {
              setPendingConfiguredMessage(content);
              setSendAfterConfiguration(true);
              setIsSettingsOpen(true);
              return;
            }
            const sent = await run(async () => {
              const mode = await sendConversationMessage(
                bridge,
                activeProject.id,
                activeTask?.id,
                content,
                activeTask?.permissionLevel ?? newConversationPermission,
              );
              if (mode === "started") setIsNewConversation(false);
            });
            if (sent) {
              setComposerDraft("");
              setAttachments([]);
            }
          }}
          onCancel={(turnId) => run(() => bridge.cancelTurn(turnId))}
          permissionLevel={activeTask?.permissionLevel ?? newConversationPermission}
          permissionScopeLabel={activeTask ? "当前对话；新对话会继承" : "用于接下来的新对话"}
          workspaceSandboxReady={snapshot.runtime.workspaceSandboxReady}
          onInstallSandbox={async () => {
            const installed = await run(() => bridge.installWorkspaceSandbox());
            if (installed) setNotice("项目沙箱已安装并通过健康检查");
            return installed;
          }}
          permissionDisabled={Boolean(snapshot.activeTurnId) || isBusy || !activeProject}
          onPermissionChange={async (permissionLevel) => {
            if (!activeTask) {
              setNewConversationPermission(permissionLevel);
              return true;
            }
            const changed = await run(() => bridge.setTaskPermission(activeTask.id, permissionLevel));
            if (changed) setNewConversationPermission(permissionLevel);
            return changed;
          }}
        />
      </section>

      {activeTask && inspectorMode ? (
        <InspectorPanel
          key={activeTask.id}
          bridge={bridge}
          taskId={activeTask.id}
          mode={inspectorMode}
          onModeChange={setInspectorMode}
          onClose={() => setInspectorMode(undefined)}
        />
      ) : null}

      {activeTask && terminalOpen ? (
        <TerminalPanel key={activeTask.id} bridge={bridge} taskId={activeTask.id} onClose={() => setTerminalOpen(false)} />
      ) : null}

      {searchOpen ? <TaskSearch snapshot={snapshot} onClose={() => setSearchOpen(false)} onSelect={(task) => { void run(async () => { await bridge.selectTask(task.id); setIsNewConversation(false); setNewConversationPermission(task.permissionLevel); setSearchOpen(false); }); }} /> : null}
      {isSettingsOpen ? (
        <ModelSettings
          onShowGuide={() => { setSendAfterConfiguration(false); setIsSettingsOpen(false); setIsGuideOpen(true); }}
          activeProfile={activeModel}
          contextUsage={activeContextUsage}
          disabled={isBusy}
          onClose={() => {
            setSendAfterConfiguration(false);
            setIsSettingsOpen(false);
          }}
          onSave={async (input) => {
            await run(async () => {
              await bridge.saveModelProfile(input);
              setIsSettingsOpen(false);
              const content = pendingConfiguredMessage;
              if (sendAfterConfiguration && content && activeProject) {
                const mode = await sendConversationMessage(
                  bridge,
                  activeProject.id,
                  activeTask?.id,
                  content,
                  activeTask?.permissionLevel ?? newConversationPermission,
                );
                if (mode === "started") setIsNewConversation(false);
                setComposerDraft("");
                setAttachments([]);
              }
              setSendAfterConfiguration(false);
            });
          }}
        />
      ) : null}
      {isAgentSettingsOpen && activeTask ? (
        <AgentSettings
          bridge={bridge}
          taskId={activeTask.id}
          onClose={() => setIsAgentSettingsOpen(false)}
        />
      ) : null}
      {isGuideOpen ? (
        <FirstRunGuide onClose={closeGuide} onConfigure={() => {
          closeGuide();
          setSendAfterConfiguration(false);
          setPendingConfiguredMessage("");
          setIsSettingsOpen(true);
        }} />
      ) : null}
    </main>
  );
}
