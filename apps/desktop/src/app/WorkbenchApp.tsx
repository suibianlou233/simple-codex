import {invoke} from "@tauri-apps/api/core";
import {ComposerQueueProvider} from "../components/ComposerQueue";
import {ConversationBoundary, ThreadPrimitive} from "../components/assistant-ui/conversation";
import {contextKey,snapshotForMode,taskMode} from "./contextModes";

import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { desktopBridge } from "../bridge";
import { sendConversationMessage } from "../conversation";
import { draftKey } from "./navigation";
import { normalizeAttachments, validateImageFiles } from "./attachmentDraft";
import { pasteIntoDraft, type PastedTextBlock } from "./textDraft";
import { Sidebar } from "../components/Sidebar";
import { TaskSearch } from "../components/TaskSearch";
import { Icon } from "../components/Icon";
import { useDesktopProjection } from "../store/entityStore";
import { TaskTimeline } from "../items/TaskTimeline";
import { Composer } from "../components/Composer";
import { AttachmentProjectContext } from "../components/StoredImage";
import { BrowserPanel } from "../panels/BrowserPanel";
import { BrowserApproval } from "../panels/BrowserApproval";
import { mediaAvailable, mediaBridge } from "../bridge/mediaBridge";
import { EmptyWorkspace } from "../components/Welcome";
import { AgentSettings } from "../settings/AgentSettings";
import { SkillManager } from "../settings/SkillManager";
import { ModelSettings } from "../settings/ModelSettings";
import { FirstRunGuide } from "../components/FirstRunGuide";
import { markGuideSeen, shouldShowGuide } from "./onboarding";
import { toMessage } from "./feedback";
import { TerminalPanel } from "../panels/TerminalPanel";
import type { TextDocument } from "../items/UserMessage";
import { CopyButton } from "../components/CopyButton";
import { WindowControls } from "../components/WindowControls";
import {lazy, Suspense} from "react";
const CodeWorkspace = lazy(() => import("../code/CodeWorkspace").then(module => ({default: module.CodeWorkspace})));
import { projectRelativePath } from "../code/editorModel";
import { FileOpenContext } from "../code/FileOpenContext";
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

export function WorkspaceNavigation({ terminalOpen, browserOpen, onTerminalToggle, onBrowserToggle, terminalDisabled = false, browserDisabled = false }: {
  terminalOpen: boolean; browserOpen: boolean;
  onTerminalToggle: () => void; onBrowserToggle: () => void;
  terminalDisabled?: boolean; browserDisabled?: boolean;
}) {
  return <nav className="sidebar-workspace-nav" aria-label="工作区工具">
    <button type="button" className={terminalOpen ? "is-active" : undefined} aria-pressed={terminalOpen} aria-label="终端" disabled={terminalDisabled} onClick={onTerminalToggle}><Icon name="terminal" size={16} /><span>终端</span></button>
    <button type="button" className={browserOpen ? "is-active" : undefined} aria-pressed={browserOpen} aria-label="浏览器" disabled={browserDisabled} onClick={onBrowserToggle}><Icon name="globe" size={16} /><span>浏览器</span></button>
  </nav>;
}

export function App({ bridge = desktopBridge }: AppProps) {
  const { snapshot, error: projectionError } = useDesktopProjection(bridge);
  const [modes, setModes] = useState<Record<string, string>>(() => {
    try { return JSON.parse(localStorage.getItem("simple.ui.projectModes") ?? "{}"); } catch { return {}; }
  });
  const modeKey = snapshot?.activeProjectId ?? "welcome";
  const codeMode = modes[modeKey] === "code";
  const [codeWasOpened,setCodeWasOpened]=useState(codeMode);
  useEffect(()=>{if(codeMode)setCodeWasOpened(true);},[codeMode]);
  const setCodeMode = (enabled: boolean) => {if(isBusy||isSettingsOpen)return;setTerminalOpen(false);setBrowserOpen(false);setTextDocument(undefined);setSearchOpen(false);setModes(previous => {
    const next = {...previous, [modeKey]: enabled ? "code" : "simple"};
    localStorage.setItem("simple.ui.projectModes", JSON.stringify(next)); return next;
  });};
  const [fileRequest, setFileRequest] = useState<{path:string;nonce:number;projectId:string;line?:number;column?:number}>();
  const [referenceRequest,setReferenceRequest]=useState(0);
  const [editorModalOpen, setEditorModalOpen] = useState(false);
  const contextMode=codeMode?"code":"simple";
  const scopeKey=contextKey(snapshot?.activeProjectId,contextMode);
  const [selections,setSelections]=useState<Record<string,string>>(()=>{try{return JSON.parse(localStorage.getItem("simple.ui.modeSelections")??"{}");}catch{return {};}});
  const [newConversations,setNewConversations]=useState<Record<string,boolean>>(()=>{try{return JSON.parse(localStorage.getItem("simple.ui.modeNewDrafts")??"{}");}catch{return {};}});
  const isNewConversation=newConversations[scopeKey]??false;
  const setIsNewConversation=(value:boolean)=>setNewConversations(previous=>({...previous,[scopeKey]:value}));
  useEffect(()=>{localStorage.setItem("simple.ui.modeNewDrafts",JSON.stringify(newConversations));},[newConversations]);
  useEffect(()=>{localStorage.setItem("simple.ui.modeSelections",JSON.stringify(selections));},[selections]);
  const matchingTasks=snapshot?.tasks.filter(t=>t.projectId===snapshot.activeProjectId&&taskMode(t)===contextMode&&!t.archived)??[];
  const selectedTask=matchingTasks.find(t=>t.id===selections[scopeKey])??matchingTasks.find(t=>t.id===snapshot?.activeTaskId)??matchingTasks[0];
  const activeTask=isNewConversation?undefined:selectedTask;
  const [isBusy, setIsBusy] = useState(false);
  const [isSkillsOpen, setIsSkillsOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isGuideOpen, setIsGuideOpen] = useState(shouldShowGuide);
  const closeGuide = () => { markGuideSeen(); setIsGuideOpen(false); };
  const [isAgentSettingsOpen, setIsAgentSettingsOpen] = useState(false);
  const [sendAfterConfiguration, setSendAfterConfiguration] = useState(false);
  const [pendingConfiguredMessage, setPendingConfiguredMessage] = useState("");
  const [drafts, setDrafts] = useState<Record<string, { content: string; attachments: AttachmentSummary[]; pastes?: PastedTextBlock[] }>>({});
  const currentDraftKey = `${draftKey(snapshot?.activeProjectId, activeTask?.id)}:${contextMode}`;
  const composerDraft = drafts[currentDraftKey]?.content ?? "";
  const attachments = drafts[currentDraftKey]?.attachments ?? [];
  const setComposerDraft = (content: string) => setDrafts((all) => ({ ...all, [currentDraftKey]: { ...all[currentDraftKey], content, attachments: all[currentDraftKey]?.attachments ?? [] } }));
  const setAttachments = (next: AttachmentSummary[] | ((current: AttachmentSummary[]) => AttachmentSummary[])) => setDrafts(all => {
    try {
      const selected = normalizeAttachments(typeof next === "function" ? next(all[currentDraftKey]?.attachments ?? []) : next);
      return {...all, [currentDraftKey]: {...all[currentDraftKey], content:all[currentDraftKey]?.content ?? "", attachments:selected}};
    } catch (error) {
      queueMicrotask(() => setError(error instanceof Error ? error.message : String(error)));
      return all;
    }
  });
  const [newConversationPermission, setNewConversationPermission] =
    useState<PermissionLevel>("approval");
  const [searchOpen, setSearchOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [notice, setNotice] = useState<string>();
  const [error, setError] = useState<string>();
  const [terminalOpen, setTerminalOpen] = useState(false);
  const [textDocument, setTextDocument] = useState<TextDocument>();
  const [terminalTasks, setTerminalTasks] = useState<string[]>([]);
  const [resizingPanel, setResizingPanel] = useState(false);
  const [windowWidth, setWindowWidth] = useState(() => typeof window === "undefined" ? 1280 : window.innerWidth);
  const [panelWidth, setPanelWidth] = useState(() => {
    if (typeof window === "undefined") return 480;
    const saved = Number(localStorage.getItem("simple.ui.rightPanelWidth"));
    return Number.isFinite(saved) && saved >= 200 ? saved : window.innerWidth * .46;
  });
  useEffect(() => { const resize = () => setWindowWidth(window.innerWidth); window.addEventListener("resize", resize); return () => window.removeEventListener("resize", resize); }, []);
  const panelMax = Math.max(200, windowWidth - (sidebarCollapsed ? 0 : windowWidth <= 1100 ? 220 : 252) - 280);
  const panelMin = Math.min(320, panelMax);
  const visiblePanelWidth = Math.round(Math.min(panelMax, Math.max(panelMin, panelWidth)));
  const resizePanel = (width: number) => {
    const next = Math.round(Math.min(panelMax, Math.max(panelMin, width)));
    setPanelWidth(next); localStorage.setItem("simple.ui.rightPanelWidth", String(next));
  };
  const [browserOpen, setBrowserOpen] = useState(false);
  const [browserApprovalOpen, setBrowserApprovalOpen] = useState(false);
  useEffect(() => {
    if (!mediaAvailable) return;
    let stopped = false; let unlisten: (() => void) | undefined;
    void listen<{projectPath:string}>("simple-browser-open",event=>{
      const active=snapshot?.projects.find(project=>project.id===snapshot.activeProjectId);
      const normalize=(path:string)=>path.replace(/\\/g,"/").toLowerCase();
      if(active && normalize(active.path)===normalize(event.payload.projectPath)) { setTextDocument(undefined); setTerminalOpen(false); setBrowserOpen(true); }
    }).then(remove=>{if(stopped)remove();else unlisten=remove;}).catch(()=>undefined);
    return ()=>{stopped=true;unlisten?.();};
  }, [snapshot?.activeProjectId, snapshot?.projects]);

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

  useEffect(()=>{
    const selected=snapshot?.tasks.find(t=>t.id===snapshot.activeTaskId);
    if(selected&&!selected.archived)setSelections(previous=>({...previous,[contextKey(selected.projectId,taskMode(selected))]:selected.id}));
  },[snapshot?.activeTaskId]);

  const activeProject = snapshot?.projects.find(
    (project) => project.id === snapshot.activeProjectId,
  );
  useEffect(() => setTextDocument(undefined), [activeTask?.id, activeProject?.id,contextMode]);
  useEffect(() => {
    if (terminalOpen && activeTask) setTerminalTasks(current => current.includes(activeTask.id) ? current : [...current, activeTask.id]);
  }, [terminalOpen, activeTask?.id]);
  const activeModel = snapshot?.modelProfiles.find(
    (profile) => profile.id === snapshot.activeModelProfileId,
  );
  const activeTimeline = useMemo(
    () =>
      snapshot?.timeline.filter((entry) => entry.taskId === activeTask?.id) ?? [],
    [snapshot,activeTask?.id],
  );
  const activeActions = useMemo(
    () => snapshot?.actions.filter((action) => action.taskId === activeTask?.id) ?? [],
    [snapshot,activeTask?.id],
  );
  const activeTurns = useMemo(
    () => snapshot?.turns.filter((turn) => turn.taskId === activeTask?.id) ?? [],
    [snapshot,activeTask?.id],
  );
  const activeTurnId=activeTurns.find(turn=>turn.status==="running")?.id??null;
  const modeSnapshot=snapshot?snapshotForMode(snapshot,contextMode):undefined;
  const activeContextUsage = snapshot?.contextUsage.find(
    (usage) => usage.taskId === activeTask?.id,
  );
  useEffect(() => {
    setFollowOutput(true);
  }, [activeTask?.id,contextMode]);
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
      if (event.defaultPrevented || (event.target instanceof Element && event.target.closest(".code-workspace"))) return;
      if (!(event.ctrlKey || event.metaKey) || event.altKey || isSettingsOpen || isSkillsOpen || isAgentSettingsOpen || isGuideOpen) return;
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
    <FileOpenContext.Provider value={activeProject ? href => {
      const path = projectRelativePath(href, activeProject.path);
      if (path) {setCodeMode(true);setFileRequest({path,line:Number(href.match(/(?:#L|:)(\d+)(?::\d+)?$/)?.[1]??1),column:Number(href.match(/:\d+:(\d+)$/)?.[1]??1),nonce:Date.now(),projectId:activeProject.id});}
    } : undefined}>
    <ConversationBoundary entries={activeTimeline} running={Boolean(snapshot.activeTurnId)}><AttachmentProjectContext.Provider value={activeProject?.id}>
    {mediaAvailable ? <BrowserApproval onVisibilityChange={setBrowserApprovalOpen} taskNames={Object.fromEntries(snapshot.tasks.map(task => [task.id, task.title]))} /> : null}
    <main style={{"--right-panel-width": `${visiblePanelWidth}px`} as CSSProperties} className={`app-shell has-mode-bar${codeMode ? " code-mode" : ""}${sidebarCollapsed ? " sidebar-collapsed" : ""}${browserOpen && activeProject ? " has-browser" : ""}${textDocument || (browserOpen && activeProject) || (terminalOpen && activeTask) ? " has-right-panel" : ""}${resizingPanel ? " is-resizing-panel" : ""}`}>
      <div className="simple-mode-bar" data-tauri-drag-region>
        <select aria-label="工作模式" disabled={isBusy||isSettingsOpen} value={codeMode ? "code" : "simple"} onChange={event=>setCodeMode(event.target.value==="code")}><option value="simple">Simple</option><option value="code">Simple Code</option></select>
        {codeMode ? <>
          <select aria-label="当前项目" value={activeProject?.id ?? ""} onChange={event=>void run(async()=>{await bridge.selectProject(event.target.value);setIsNewConversation(true);})}><option value="" disabled>选择项目</option>{snapshot.projects.map(project=><option key={project.id} value={project.id}>{project.name}</option>)}</select>
          <button onClick={()=>void run(async()=>{await bridge.openProject();setIsNewConversation(true);})}>打开文件夹</button>
          <span className="window-drag-spacer" data-tauri-drag-region aria-hidden="true"/>
          <nav className="code-tools" aria-label="右侧工具"><button aria-pressed={!terminalOpen&&!browserOpen&&!textDocument} onClick={()=>{setTerminalOpen(false);setBrowserOpen(false);setTextDocument(undefined);}}>AI</button><WorkspaceNavigation terminalOpen={terminalOpen} browserOpen={browserOpen} terminalDisabled={!activeTask} browserDisabled={!activeProject||!mediaAvailable} onTerminalToggle={()=>{setTextDocument(undefined);setBrowserOpen(false);setTerminalOpen(v=>!v);}} onBrowserToggle={()=>{setTextDocument(undefined);setTerminalOpen(false);setBrowserOpen(v=>!v);}}/></nav>
        </> : <><span data-tauri-drag-region>{activeProject?.name ?? "本地工作区"}</span><span className="window-drag-spacer" data-tauri-drag-region aria-hidden="true"/></>}
        <WindowControls onError={setError}/>
      </div>
      {codeMode || codeWasOpened ? <Suspense fallback={<section className="code-workspace" hidden={!codeMode}><div className="code-empty">加载编辑器…</div></section>}><CodeWorkspace project={activeProject} visible={codeMode} dark={resolvedTheme==="dark"} request={fileRequest?.projectId===activeProject?.id?fileRequest:undefined} onModalVisibility={setEditorModalOpen} referenceRequest={referenceRequest} profileId={activeModel?.id} bridge={bridge} taskId={activeTask?.id} actions={activeActions} turns={activeTurns}
        onOpenProject={()=>void run(async()=>{await bridge.openProject();setIsNewConversation(true);})}
        onAttach={block=>{setDrafts(all=>{const current=all[currentDraftKey];return {...all,[currentDraftKey]:{content:current?.content??"",attachments:current?.attachments??[],pastes:[...(current?.pastes??[]),block]}};});setTerminalOpen(false);setBrowserOpen(false);setTextDocument(undefined);}}/></Suspense> : null}
      {codeMode ? <div className="code-panel-resizer" role="separator" aria-label="调整 AI 侧栏宽度" aria-orientation="vertical" tabIndex={0}
        onPointerDown={event=>{if(event.button!==0)return;event.preventDefault();event.currentTarget.setPointerCapture(event.pointerId);setResizingPanel(true);}}
        onPointerMove={event=>{if(event.currentTarget.hasPointerCapture(event.pointerId))resizePanel(window.innerWidth-event.clientX);}}
        onPointerUp={event=>{event.currentTarget.releasePointerCapture(event.pointerId);setResizingPanel(false);}}
        onLostPointerCapture={()=>setResizingPanel(false)} onKeyDown={event=>{if(event.key==="ArrowLeft"||event.key==="ArrowRight"){event.preventDefault();resizePanel(visiblePanelWidth+(event.key==="ArrowLeft"?24:-24));}}}/> : null}
      {!sidebarCollapsed ? <Sidebar snapshot={modeSnapshot!} activeTaskId={activeTask?.id} busy={isBusy}
        operationError={error}
        onArchive={(task, archived) => run(async () => { await bridge.setTaskArchived(task.id, archived); setNotice(archived ? "对话已归档" : "对话已恢复"); })}
        onDelete={(task) => run(async () => {
          await bridge.deleteTask(task.id);
          setDrafts(all => { const next = { ...all }; delete next[draftKey(task.projectId, task.id)]; return next; });
          setTerminalTasks(tasks => tasks.filter(id => id !== task.id));
          setNotice("对话已删除");
        })}
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

      <ThreadPrimitive.Root asChild><section className={`workspace${activeTask ? "" : " workspace-welcome"}`}>
        {codeMode?<header className="code-chat-header"><span className="code-chat-tab">Agent</span><div className="code-chat-session">          <select aria-label="当前对话" value={activeTask?.id ?? ""} onChange={event=>{if(!event.target.value)beginNewConversation();else void run(async()=>{await bridge.selectTask(event.target.value);setIsNewConversation(false);});}}><option value="">新对话</option>{modeSnapshot!.tasks.filter(task=>task.projectId===activeProject?.id&&!task.archived).map(task=><option key={task.id} value={task.id}>{task.title}</option>)}</select>
          <button onClick={beginNewConversation} aria-label="新建代码对话" title="新建对话">＋</button>
</div></header>:<header className="workspace-header">
          <div className="workspace-heading">
            {sidebarCollapsed ? <button className="icon-button" onClick={() => setSidebarCollapsed(false)} aria-label="展开侧栏" title="展开侧栏（Ctrl/⌘+B）"><Icon name="sidebar" /></button> : null}
            <span className={`workspace-status${activeTask ? ` status-${activeTask.status}` : ""}`} aria-hidden="true" />
            <div>
              <strong title={activeTask?.title}>{activeTask?.title ?? (activeProject ? "新对话" : "Simple")}</strong>
              <small title={activeProject?.path}>{activeProject?.name ?? "本地工作区"}<span className="breadcrumb-divider">/</span>{activeTask ? "任务" : "开始新任务"}</small>
            </div>
          </div>
          {activeProject ? <WorkspaceNavigation terminalOpen={terminalOpen} browserOpen={browserOpen}
            terminalDisabled={!activeTask} browserDisabled={!mediaAvailable}
            onTerminalToggle={() => { setTextDocument(undefined); setBrowserOpen(false); setTerminalOpen(value => !value); }}
            onBrowserToggle={() => { setTextDocument(undefined); setTerminalOpen(false); setBrowserOpen(value => !value); }} /> : null}
        </header>}

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
        <ThreadPrimitive.Viewport autoScroll={false}
          className="timeline aui-thread-viewport"
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
              onOpenText={document => {setTextDocument(document);setBrowserOpen(false);setTerminalOpen(false);}}
              actions={activeActions}
              activeTurnId={activeTurnId}
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
          ) : codeMode ? (
            <div className="code-chat-empty"><strong>开始一个开发任务</strong><p>提问、修改代码，或用 @ 引用项目文件。</p><span>Ctrl / ⌘ K 修改选区</span></div>
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
        </ThreadPrimitive.Viewport>
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

        <ComposerQueueProvider key={currentDraftKey} taskId={activeTask?.id} bridge={bridge} turns={activeTurns} onSteer={(turnId,content)=>invoke("steer_turn",{turnId,content})}>
        <Composer
          onQueued={() => {const submitted = drafts[currentDraftKey]; setDrafts(all => all[currentDraftKey] !== submitted ? all : {...all, [currentDraftKey]: {content: "", attachments: [], pastes: []}});}}
          variant={codeMode?"code":"simple"}
          key={currentDraftKey}
          onReference={codeMode?()=>setReferenceRequest(v=>v+1):undefined}
          pastes={drafts[currentDraftKey]?.pastes ?? []}
          onTextPaste={(text, start, end) => setDrafts(all => {
            const current = all[currentDraftKey];
            const pasted = pasteIntoDraft({content: current?.content ?? "", pastes: current?.pastes ?? []}, text, start, end, crypto.randomUUID());
            return {...all, [currentDraftKey]: {...pasted, attachments: current?.attachments ?? []}};
          })}
          onEditPaste={(id, text) => setDrafts(all => {
            const current = all[currentDraftKey];
            if (!current) return all;
            return {...all, [currentDraftKey]: {...current, pastes: current.pastes?.map(block => block.id === id ? {...block, text, source:block.label?"手动编辑的引用快照":undefined} : block)}};
          })}
          onRemovePaste={id => setDrafts(all => {
            const current = all[currentDraftKey];
            if (!current) return all;
            return {...all, [currentDraftKey]: {...current, pastes: current.pastes?.filter(block => block.id !== id)}};
          })}
          projectId={activeProject?.id}
          projectName={activeProject?.name}
          modelProfiles={snapshot.modelProfiles}
          activeModelId={snapshot.activeModelProfileId}
          onModelChange={(id) => run(() => bridge.selectModelProfile(id))}
          onModelSettings={() => { setSendAfterConfiguration(false); setIsSettingsOpen(true); }}
          contextUsage={activeContextUsage}
          phase={snapshot.turns.find((turn) => turn.id === activeTurnId)?.phase}
          activeTurnId={activeTurnId}
          disabled={isBusy}
          content={composerDraft}
          attachments={attachments}
          onContentChange={setComposerDraft}
          onImages={mediaAvailable ? async () => {
            if (!activeProject) return;
            await run(async () => {
              const selected = await mediaBridge.pickImages(activeProject.id);
              setAttachments(current => [...new Map([...current, ...selected].map(item => [item.path, item])).values()]);
            });
          } : undefined}
          onPasteImages={mediaAvailable ? async files => {
            if (!activeProject) return;
            await run(async () => {
              validateImageFiles(files);
              const selected: AttachmentSummary[] = [];
              for (const file of files) selected.push(await mediaBridge.pasteImage(activeProject.id, file));
              setAttachments(current => [...new Map([...current, ...selected].map(item => [item.path, item])).values()]);
            });
          } : undefined}
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
                contextMode,
              );
              if (mode === "started") setIsNewConversation(false);
            });
            if (sent) {
              setDrafts(all => ({...all, [currentDraftKey]: {content: "", attachments: [], pastes: []}}));
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
          permissionDisabled={Boolean(activeTurnId) || isBusy || !activeProject}
          onPermissionChange={async (permissionLevel) => {
            if (!activeTask) {
              setNewConversationPermission(permissionLevel);
              return true;
            }
            const changed = await run(() => bridge.setTaskPermission(activeTask.id, permissionLevel));
            if (changed) setNewConversationPermission(permissionLevel);
            return changed;
          }}
        /></ComposerQueueProvider>
      </section></ThreadPrimitive.Root>

      {textDocument || (activeProject && browserOpen) || terminalTasks.length > 0 ? <aside className="right-panel" hidden={!textDocument && !browserOpen && !terminalOpen} aria-label="右侧工作区">
        <div className="right-panel-resizer" role="separator" aria-label="调整右侧栏宽度" aria-orientation="vertical" tabIndex={0}
          aria-valuemin={panelMin} aria-valuemax={panelMax} aria-valuenow={visiblePanelWidth} title="拖动调整宽度，双击恢复默认"
          onDoubleClick={()=>resizePanel(windowWidth * .46)}
          onPointerDown={event=>{if(event.button!==0)return;event.preventDefault();event.currentTarget.setPointerCapture(event.pointerId);setResizingPanel(true);}}
          onPointerMove={event=>{if(event.currentTarget.hasPointerCapture(event.pointerId)) resizePanel(window.innerWidth-event.clientX);}}
          onPointerUp={event=>{if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);setResizingPanel(false);}}
          onPointerCancel={()=>setResizingPanel(false)} onLostPointerCapture={()=>setResizingPanel(false)}
          onKeyDown={event=>{if(event.key==='ArrowLeft'||event.key==='ArrowRight'){event.preventDefault();resizePanel(visiblePanelWidth+(event.key==='ArrowLeft'?24:-24));}else if(event.key==='Home'||event.key==='End'){event.preventDefault();resizePanel(event.key==='Home'?panelMin:panelMax);}}} />
        {textDocument ? <section className="text-reader" aria-label="正文阅读栏">
          <header><div><strong title={textDocument.title}>{textDocument.title}</strong><small>{Array.from(textDocument.text).length.toLocaleString()} 字符</small></div>
            <CopyButton text={textDocument.text} label="复制正文"/><button type="button" className="icon-button" aria-label="关闭正文" onClick={()=>setTextDocument(undefined)}><Icon name="close" size={18}/></button>
          </header>
          <pre tabIndex={0} aria-label="正文全文">{textDocument.text}</pre>
        </section> : null}
        {activeProject && browserOpen ? <BrowserPanel key={activeProject.id} projectId={activeProject.id}
          suspended={isSettingsOpen || isGuideOpen || isSkillsOpen || isAgentSettingsOpen || searchOpen || browserApprovalOpen || resizingPanel || editorModalOpen}
          onClose={()=>setBrowserOpen(false)}
          onAttach={attachment => setAttachments(current => current.some(item => item.path === attachment.path) ? current : [...current, attachment])} /> : null}
        {terminalTasks.map(taskId => <div key={taskId} className="terminal-slot" hidden={!terminalOpen || taskId !== activeTask?.id}>
          <TerminalPanel onSendError={text=>{setDrafts(all=>{const current=all[currentDraftKey];return {...all,[currentDraftKey]:{content:current?.content??"",attachments:current?.attachments??[],pastes:[...(current?.pastes??[]),{id:crypto.randomUUID(),label:"终端报错",source:"终端选区",text}]}};});setTerminalOpen(false);setBrowserOpen(false);setTextDocument(undefined);}} taskId={taskId} onClose={() => setTerminalOpen(false)} />
        </div>)}
      </aside> : null}

      {searchOpen ? <TaskSearch snapshot={modeSnapshot!} onClose={() => setSearchOpen(false)} onSelect={(task) => { void run(async () => { await bridge.selectTask(task.id); setIsNewConversation(false); setNewConversationPermission(task.permissionLevel); setSearchOpen(false); }); }} /> : null}
      {isSettingsOpen ? (
        <ModelSettings
          onManageSkills={() => { setIsSettingsOpen(false); setIsSkillsOpen(true); }}
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
                contextMode,
                );
                if (mode === "started") setIsNewConversation(false);
                setDrafts(all => ({...all, [currentDraftKey]: {content: "", attachments: [], pastes: []}}));
              }
              setSendAfterConfiguration(false);
            });
          }}
        />
      ) : null}
      {isSkillsOpen && <SkillManager onClose={() => setIsSkillsOpen(false)} />}
      {isAgentSettingsOpen && activeTask ? (
        <AgentSettings
          onManageSkills={() => { setIsAgentSettingsOpen(false); setIsSkillsOpen(true); }}
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
    </AttachmentProjectContext.Provider></ConversationBoundary>
    </FileOpenContext.Provider>
  );
}
