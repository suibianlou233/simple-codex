import {useComposerQueue, ComposerQueueTray} from "./ComposerQueue";
import {ComposerPrimitive, useAui} from "@assistant-ui/react";
import {ConversationBoundary, useConversationActions} from "./assistant-ui/conversation";
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import type { AttachmentSummary, ContextUsage, ModelProfileSummary, PermissionLevel, TurnSummary } from "../bridge/types";
import { composerHeight, shouldSubmitMessage } from "../app/interactions";
import { Icon } from "./Icon";
import { PermissionSelector } from "./PermissionSelector";
import { StoredImage } from "./StoredImage";
import { buildComposerMessage } from "../app/attachmentDraft";
import { composeTextDraft, LARGE_PASTE_CHAR_THRESHOLD, type PastedTextBlock } from "../app/textDraft";
import { rememberSentText } from "../app/sentText";
import { useDialog } from "./useDialog";

export function Composer(props: Parameters<typeof ComposerContent>[0]) {return <ConversationBoundary running={Boolean(props.activeTurnId)}><ComposerContent {...props}/></ConversationBoundary>;}
function ComposerContent({
  variant = "simple",
  projectId, projectName, modelProfiles = [], activeModelId, onModelChange, onModelSettings, contextUsage, phase,
  activeTurnId,
  disabled,
  content,
  pastes = [], onTextPaste, onRemovePaste, onEditPaste,
  attachments,
  onContentChange,
  onAttach, onReference,
  onImages,
  onPasteImages,
  onRemoveAttachment,
  onSend, onQueued,
  onCancel,
  permissionLevel,
  permissionScopeLabel,
  workspaceSandboxReady,
  onInstallSandbox,
  permissionDisabled,
  onPermissionChange,
}: {
  variant?: "simple" | "code";
  projectId?: string;
  projectName?: string;
  modelProfiles?: ModelProfileSummary[];
  activeModelId?: string | null;
  onModelChange?: (id: string) => Promise<boolean>;
  onModelSettings?: () => void;
  contextUsage?: ContextUsage;
  phase?: TurnSummary["phase"];
  activeTurnId: string | null;
  disabled: boolean;
  content: string;
  pastes?: PastedTextBlock[];
  onTextPaste?: (text: string, start: number, end: number) => void;
  onRemovePaste?: (id: string) => void;
  onEditPaste?: (id: string, text: string) => void;
  attachments: AttachmentSummary[];
  onContentChange: (content: string) => void;
  onAttach: () => Promise<void>;
  onReference?: () => void;
  onImages?: () => Promise<void>;
  onPasteImages?: (files: File[]) => Promise<void>;
  onRemoveAttachment: (path: string) => void;
  onSend: (content: string) => Promise<void>;
  onQueued?: () => void;
  onCancel: (turnId: string) => Promise<boolean>;
  permissionLevel: PermissionLevel;
  permissionScopeLabel: string;
  workspaceSandboxReady: boolean;
  onInstallSandbox?: () => Promise<boolean>;
  permissionDisabled: boolean;
  onPermissionChange: (permissionLevel: PermissionLevel) => Promise<boolean>;
}) {
  const queue = useComposerQueue();
  const canQueue = Boolean(queue?.canQueue && onQueued);
  const inputLocked = Boolean(activeTurnId) && !canQueue;
  const [submitError, setSubmitError] = useState("");
  const aui = useAui();
  const conversationActions = useConversationActions();
  useEffect(() => {aui.composer().setText(content);}, [aui, content]);
  const [cancelRequestedTurnId, setCancelRequestedTurnId] = useState<string>();
  const [draggingImages, setDraggingImages] = useState(false);
  const [editingPaste, setEditingPaste] = useState<PastedTextBlock>();
  const pasteCounts = useMemo(() => new Map(pastes.map(block => [block.id, Array.from(block.text).length.toLocaleString()])), [pastes]);
  const dragDepth = useRef(0);
  const canAddImages = Boolean(projectId && !inputLocked && !disabled && onPasteImages);
  useEffect(() => {
    const preventFileNavigation = (event: DragEvent) => {
      if (Array.from(event.dataTransfer?.types ?? []).includes("Files")) event.preventDefault();
    };
    window.addEventListener("dragover", preventFileNavigation);
    window.addEventListener("drop", preventFileNavigation);
    return () => {
      window.removeEventListener("dragover", preventFileNavigation);
      window.removeEventListener("drop", preventFileNavigation);
    };
  }, []);
  useEffect(() => { dragDepth.current = 0; setDraggingImages(false); }, [projectId, activeTurnId, disabled]);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const composingRef = useRef(false);
  const submittingRef = useRef(false);
  useEffect(() => {
    const input = textareaRef.current;
    if (!input) return;
    const resize = () => {
      input.style.height = "auto";
      input.style.height = `${composerHeight(input.scrollHeight)}px`;
      input.style.overflowY = input.scrollHeight > input.clientHeight ? "auto" : "hidden";
    };
    resize();
    if (typeof ResizeObserver === "undefined") return;
    let width = input.clientWidth;
    const observer = new ResizeObserver(() => {
      if (input.clientWidth === width) return;
      width = input.clientWidth;
      resize();
    });
    observer.observe(input);
    return () => observer.disconnect();
  }, [content]);
  useEffect(() => {
    if (cancelRequestedTurnId && activeTurnId !== cancelRequestedTurnId) {
      setCancelRequestedTurnId(undefined);
    }
  }, [activeTurnId, cancelRequestedTurnId]);
  const cancelRequested = Boolean(
    activeTurnId && cancelRequestedTurnId === activeTurnId,
  );
  const canSend = Boolean(
    projectId && (content.trim() || pastes.length > 0 || attachments.length > 0) && !inputLocked && !disabled && !queue?.busy,
  );
  const send = async (steer = false) => {
    if (!projectId || !canSend || submittingRef.current) return;
    const message = buildComposerMessage(composeTextDraft(content, pastes), attachments);
    submittingRef.current = true; setSubmitError("");
    try {
      await rememberSentText(message, content, pastes);
      if (canQueue && queue) {const accepted = steer ? await queue.steer(message) : queue.enqueue(message); if (accepted) onQueued?.();}
      else await onSend(message);
    } catch (error) {setSubmitError(String(error));}
    finally {submittingRef.current = false;}
  };
  const submit = (event: FormEvent<HTMLFormElement>) => {event.preventDefault(); void send();};
  useEffect(() => {if (!conversationActions) return; const actions = {send, cancel: async () => {if (activeTurnId) await onCancel(activeTurnId);}}; conversationActions.current = actions; return () => {if (conversationActions.current === actions) conversationActions.current = null;};});
  const usagePercent = contextUsage && contextUsage.contextWindowTokens > 0 ? Math.min(100, Math.round(contextUsage.estimatedTokens / contextUsage.contextWindowTokens * 100)) : undefined;
  return (
    <footer className="composer-shell">
      <ComposerQueueTray/>
      {submitError ? <p role="alert">{submitError}</p> : null}
      <ComposerPrimitive.Root className={`composer${draggingImages ? " composer-dragging" : ""}`} onSubmit={submit}
        onDragEnter={event => {
          if (!Array.from(event.dataTransfer.types).includes("Files")) return;
          event.preventDefault();
          dragDepth.current += 1;
          if (canAddImages) setDraggingImages(true);
        }}
        onDragOver={event => {
          if (!Array.from(event.dataTransfer.types).includes("Files")) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = canAddImages ? "copy" : "none";
        }}
        onDragLeave={() => {
          dragDepth.current = Math.max(0, dragDepth.current - 1);
          if (dragDepth.current === 0) setDraggingImages(false);
        }}
        onDrop={event => {
          if (!Array.from(event.dataTransfer.types).includes("Files")) return;
          event.preventDefault();
          dragDepth.current = 0;
          setDraggingImages(false);
          if (!canAddImages) return;
          // The existing attachment command validates file bytes and size.
          const files = Array.from(event.dataTransfer.files);
          if (files.length) void onPasteImages?.(files);
        }}>
        {draggingImages ? <div className="composer-drop-hint" role="status"><Icon name="image" size={24} /><span>松开添加图片</span><small>PNG、JPG、GIF、WebP · 每张不超过 4 MiB</small></div> : null}
        {variant==="code"?<div className="code-composer-context"><button type="button" disabled={!projectId||disabled||inputLocked} onClick={onReference} title="引用项目文件">@ 添加上下文</button><span>{projectName}</span></div>:null}
        {attachments.length > 0 ? <div className="attachment-list" aria-label="待发送附件">{attachments.map((attachment) => <span key={attachment.path}>{attachment.path.startsWith("simple-image:") ? <StoredImage reference={attachment.path} name={attachment.name} projectId={projectId} /> : <Icon name="code" size={14} />}{attachment.name}<button type="button" aria-label={`移除 ${attachment.name}`} onKeyDown={event => {
          if (event.key === "Delete" || event.key === "Backspace") { event.preventDefault(); onRemoveAttachment(attachment.path); textareaRef.current?.focus(); }
          if (event.key === "ArrowDown" || event.key === "Escape") { event.preventDefault(); textareaRef.current?.focus(); }
        }} onClick={() => onRemoveAttachment(attachment.path)}><Icon name="close" size={12} /></button></span>)}</div> : null}
        {pastes.length > 0 ? <div className="pasted-text-list" aria-label="已粘贴文本">{pastes.map((block, index) => <div className="pasted-text-chip" key={block.id}>
          <button className="pasted-text-open" type="button" aria-label={`查看粘贴文本 ${index + 1}`} onClick={() => setEditingPaste(block)}>
            <span className="pasted-text-icon"><Icon name="text" size={19} /></span>
            <span className="pasted-text-label"><strong>{block.label ?? `粘贴文本${pastes.length>1?` ${index+1}`:""}`}</strong><small>{block.source ? `${block.source} · ` : ""}{pasteCounts.get(block.id)} 字符</small></span>
          </button>
          <button className="pasted-text-remove" type="button" aria-label={`移除粘贴文本 ${index + 1}`} disabled={disabled || inputLocked} onClick={() => { onRemovePaste?.(block.id); textareaRef.current?.focus(); }}><Icon name="close" size={13} /></button>
        </div>)}</div> : null}
        <ComposerPrimitive.Input submitMode="none" cancelOnEscape={false} addAttachmentOnPaste={false} unstable_focusOnRunStart={false} unstable_focusOnThreadSwitched={false}
          ref={textareaRef}
          rows={1}
          aria-label="给 Simple 的任务"
          aria-describedby="composer-help"
          value={content}
          disabled={!projectId || inputLocked || disabled}
          placeholder={canQueue ? "继续输入任务，Enter 加入队列…" : !projectId ? "先打开一个本地项目" : activeTurnId ? phase === "checking_submission" || phase === "submission_recovery_required" ? "任务状态待确认，暂时不能发送新指令" : "任务进行中，完成后可以继续提问" : variant==="code" ? "提问、规划或修改代码…" : "描述你想完成的任务，或添加项目文件…"}
          onCompositionStart={() => { composingRef.current = true; }}
          onCompositionEnd={() => { composingRef.current = false; }}
          onChange={(event) => onContentChange(event.target.value)}
          onPaste={event => {
            const images = Array.from(event.clipboardData.files).filter(file => file.type.startsWith("image/"));
            if (images.length && canAddImages && onPasteImages) { event.preventDefault(); void onPasteImages(images); return; }
            const text = event.clipboardData.getData("text/plain");
            if (!text) return;
            event.preventDefault();
            const {selectionStart: start, selectionEnd: end} = event.currentTarget;
            const collapse = Boolean(onTextPaste && Array.from(text).length > LARGE_PASTE_CHAR_THRESHOLD);
            if (onTextPaste) onTextPaste(text, start, end);
            else onContentChange(content.slice(0, start) + text + content.slice(end));
            const cursor = collapse ? 0 : start + text.length;
            requestAnimationFrame(() => { textareaRef.current?.focus(); textareaRef.current?.setSelectionRange(cursor, cursor); });
          }}
          onKeyDown={(event) => {
            if(event.key==="@" && onReference && !event.nativeEvent.isComposing && !disabled && !inputLocked){event.preventDefault();onReference();return;}
            if (event.key === "ArrowUp" && event.currentTarget.selectionStart === 0 && event.currentTarget.selectionEnd === 0 && attachments.length && !event.nativeEvent.isComposing) {
              const buttons = event.currentTarget.form?.querySelectorAll<HTMLButtonElement>(".attachment-list button");
              if (buttons?.length) { event.preventDefault(); buttons[buttons.length - 1].focus(); return; }
            }
            if (shouldSubmitMessage({
              key: event.key,
              shiftKey: event.shiftKey,
              isComposing: composingRef.current || event.nativeEvent.isComposing,
              keyCode: event.nativeEvent.keyCode,
            })) {
              event.preventDefault();
              event.currentTarget.form?.requestSubmit();
            }
          }}
        />

        <div className="composer-toolbar">
          {onReference&&variant!=="code"?<button type="button" className="attach-button icon-button" aria-label="引用项目文件" title="引用项目文件（@）" disabled={!projectId||disabled||!!activeTurnId} onClick={onReference}>@</button>:null}
          <button className="attach-button icon-button" type="button" disabled={!projectId || inputLocked || disabled} aria-label="添加项目文件" title="添加项目文件" onClick={() => void onAttach()}><Icon name="plus" size={20} /></button>
          {onImages ? <button className="attach-button icon-button" type="button" disabled={!projectId || inputLocked || disabled} aria-label="添加图片" onClick={() => void onImages()} title="添加图片 · 也可直接粘贴截图"><Icon name="image" size={18} /></button> : null}
          <div className="composer-model">
            {modelProfiles.length ? <select aria-label="选择模型" title="当前模型配置" value={activeModelId ?? ""} disabled={disabled || Boolean(activeTurnId)} onChange={(event) => { if (event.target.value === "__settings__") onModelSettings?.(); else void onModelChange?.(event.target.value); }}>
              {!activeModelId ? <option value="" disabled>选择模型</option> : null}
              {modelProfiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.name} · {profile.model}</option>)}
              <option value="__settings__">配置模型…</option>
            </select> : <button type="button" onClick={onModelSettings} disabled={disabled || Boolean(activeTurnId)}><Icon name="spark" size={14} />配置模型</button>}
          </div>
      <PermissionSelector
        value={permissionLevel}
        disabled={permissionDisabled}
        disabledMessage={
          !projectId
            ? "打开项目后可选择权限"
            : activeTurnId
              ? "任务运行时不可切换"
              : disabled
                ? "当前操作完成后可切换"
                : undefined
        }
        scopeLabel={permissionScopeLabel}
        workspaceSandboxReady={workspaceSandboxReady}
        onInstallSandbox={onInstallSandbox}
        onChange={onPermissionChange}
      />

          <div className="composer-spacer" />
        {canQueue ? <><button type="submit" className="queue-send" disabled={!canSend} aria-label="加入队列" title="当前任务结束后依次执行">加入队列</button>{queue?.canSteer ? <button type="button" className="queue-steer" disabled={!canSend} onClick={() => void send(true)} title="立即补充当前任务">立即补充</button> : null}</> : null}
        {activeTurnId ? (
          <button
            className="stop-button"
            type="button"
            disabled={cancelRequested}
            aria-label={cancelRequested ? "正在停止回复" : "停止回复"}
            title={cancelRequested ? "已请求停止，等待内核结束" : "停止当前任务"}
            onClick={() => {
              setCancelRequestedTurnId(activeTurnId);
              void onCancel(activeTurnId).then((requested) => {
                if (!requested) setCancelRequestedTurnId(undefined);
              });
            }}
          >
            <span className="stop-square" />
          </button>
        ) : (
          <button type="submit" disabled={!canSend} aria-label="发送" title="发送消息（Enter）">
            <Icon name="arrow" size={18} />
          </button>
        )}

        </div>
      </ComposerPrimitive.Root>
      <div className="composer-meta">
        <span id="composer-help">{canQueue ? "Enter 加入队列 · Shift + Enter 换行" : activeTurnId ? "执行记录保存在本地" : attachments.some(item => item.path.startsWith("simple-image:")) && modelProfiles.find(profile => profile.id === activeModelId)?.dialect === "deep_seek" ? "图片将自动交给 DeepSeek 视觉模型处理" : "Enter 发送 · Shift + Enter 换行"}</span>
        <span className="composer-project" title={projectName}><Icon name="folder" size={12} />{projectName ?? "尚未选择项目"}</span>
        {usagePercent !== undefined ? <span className="context-meter" title={`估算上下文：${contextUsage?.estimatedTokens.toLocaleString()} / ${contextUsage?.contextWindowTokens.toLocaleString()} tokens；不是计费数据`}><meter min={0} max={100} value={usagePercent} aria-label="估算上下文使用比例" />{usagePercent}%</span> : null}
      </div>
      {editingPaste ? <PastedTextEditor key={editingPaste.id} block={editingPaste} disabled={disabled || inputLocked} onClose={() => setEditingPaste(undefined)} onSave={text => { onEditPaste?.(editingPaste.id, text); setEditingPaste(undefined); requestAnimationFrame(() => textareaRef.current?.focus()); }} /> : null}
    </footer>
  );
}

function PastedTextEditor({block, disabled, onClose, onSave}: {block: PastedTextBlock; disabled: boolean; onClose: () => void; onSave: (text: string) => void}) {
  const [text, setText] = useState(block.text);
  const ref = useDialog(onClose);
  return <div className="modal-backdrop pasted-text-backdrop" onMouseDown={onClose}><section className="pasted-text-dialog" ref={ref} role="dialog" aria-modal="true" aria-label="粘贴文本" tabIndex={-1} onMouseDown={event => event.stopPropagation()}>
    <header><div><strong>粘贴文本</strong><span>{Array.from(text).length.toLocaleString()} 字符</span></div><button type="button" className="icon-button" aria-label="关闭文本预览" onClick={onClose}><Icon name="close" size={18} /></button></header>
    <textarea aria-label="编辑粘贴文本" value={text} onChange={event => setText(event.target.value)} readOnly={disabled} spellCheck={false} />
    <footer><button className="paste-edit-cancel" type="button" onClick={onClose}>取消</button><button className="paste-edit-save" type="button" disabled={disabled || !text.trim()} onClick={() => onSave(text)}>保存</button></footer>
  </section></div>;
}
