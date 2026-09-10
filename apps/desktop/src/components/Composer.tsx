import { useEffect, useRef, useState, type FormEvent } from "react";
import type { AttachmentSummary, ContextUsage, ModelProfileSummary, PermissionLevel, TurnSummary } from "../bridge/types";
import { composerHeight, shouldSubmitMessage } from "../app/interactions";
import { Icon } from "./Icon";
import { PermissionSelector } from "./PermissionSelector";
import { StoredImage } from "./StoredImage";
import { buildComposerMessage } from "../app/attachmentDraft";

export function Composer({
  projectId, projectName, modelProfiles = [], activeModelId, onModelChange, onModelSettings, contextUsage, phase,
  activeTurnId,
  disabled,
  content,
  attachments,
  onContentChange,
  onAttach,
  onImages,
  onPasteImages,
  onRemoveAttachment,
  onSend,
  onCancel,
  permissionLevel,
  permissionScopeLabel,
  workspaceSandboxReady,
  onInstallSandbox,
  permissionDisabled,
  onPermissionChange,
}: {
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
  attachments: AttachmentSummary[];
  onContentChange: (content: string) => void;
  onAttach: () => Promise<void>;
  onImages?: () => Promise<void>;
  onPasteImages?: (files: File[]) => Promise<void>;
  onRemoveAttachment: (path: string) => void;
  onSend: (content: string) => Promise<void>;
  onCancel: (turnId: string) => Promise<boolean>;
  permissionLevel: PermissionLevel;
  permissionScopeLabel: string;
  workspaceSandboxReady: boolean;
  onInstallSandbox?: () => Promise<boolean>;
  permissionDisabled: boolean;
  onPermissionChange: (permissionLevel: PermissionLevel) => Promise<boolean>;
}) {
  const [cancelRequestedTurnId, setCancelRequestedTurnId] = useState<string>();
  const [draggingImages, setDraggingImages] = useState(false);
  const dragDepth = useRef(0);
  const canAddImages = Boolean(projectId && !activeTurnId && !disabled && onPasteImages);
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
    projectId && (content.trim() || attachments.length > 0) && !activeTurnId && !disabled,
  );
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!projectId || !canSend) return;
    const message = buildComposerMessage(content, attachments);
    void onSend(message);
  };
  const usagePercent = contextUsage && contextUsage.contextWindowTokens > 0 ? Math.min(100, Math.round(contextUsage.estimatedTokens / contextUsage.contextWindowTokens * 100)) : undefined;
  return (
    <footer className="composer-shell">
      <form className={`composer${draggingImages ? " composer-dragging" : ""}`} onSubmit={submit}
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
        {attachments.length > 0 ? <div className="attachment-list" aria-label="待发送附件">{attachments.map((attachment) => <span key={attachment.path}>{attachment.path.startsWith("simple-image:") ? <StoredImage reference={attachment.path} name={attachment.name} projectId={projectId} /> : <Icon name="code" size={14} />}{attachment.name}<button type="button" aria-label={`移除 ${attachment.name}`} onKeyDown={event => {
          if (event.key === "Delete" || event.key === "Backspace") { event.preventDefault(); onRemoveAttachment(attachment.path); textareaRef.current?.focus(); }
          if (event.key === "ArrowDown" || event.key === "Escape") { event.preventDefault(); textareaRef.current?.focus(); }
        }} onClick={() => onRemoveAttachment(attachment.path)}><Icon name="close" size={12} /></button></span>)}</div> : null}
        <textarea
          ref={textareaRef}
          rows={1}
          aria-label="给 Simple 的任务"
          aria-describedby="composer-help"
          value={content}
          disabled={!projectId || Boolean(activeTurnId) || disabled}
          placeholder={!projectId ? "先打开一个本地项目" : activeTurnId ? phase === "checking_submission" || phase === "submission_recovery_required" ? "任务状态待确认，暂时不能发送新指令" : "任务进行中，完成后可以继续提问" : "描述你想完成的任务，或添加项目文件…"}
          onCompositionStart={() => { composingRef.current = true; }}
          onCompositionEnd={() => { composingRef.current = false; }}
          onChange={(event) => onContentChange(event.target.value)}
          onPaste={event => {
            const images = Array.from(event.clipboardData.files).filter(file => file.type.startsWith("image/"));
            if (images.length && canAddImages && onPasteImages) { event.preventDefault(); void onPasteImages(images); }
          }}
          onKeyDown={(event) => {
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
          <button className="attach-button icon-button" type="button" disabled={!projectId || Boolean(activeTurnId) || disabled} aria-label="添加项目文件" title="添加项目文件" onClick={() => void onAttach()}><Icon name="plus" size={20} /></button>
          {onImages ? <button className="attach-button icon-button" type="button" disabled={!projectId || Boolean(activeTurnId) || disabled} aria-label="添加图片" onClick={() => void onImages()} title="添加图片 · 也可直接粘贴截图"><Icon name="image" size={18} /></button> : null}
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
      </form>
      <div className="composer-meta">
        <span id="composer-help">{activeTurnId ? "执行记录保存在本地" : attachments.some(item => item.path.startsWith("simple-image:")) && modelProfiles.find(profile => profile.id === activeModelId)?.dialect === "deep_seek" ? "图片将自动交给 DeepSeek 视觉模型处理" : "Enter 发送 · Shift + Enter 换行"}</span>
        <span className="composer-project" title={projectName}><Icon name="folder" size={12} />{projectName ?? "尚未选择项目"}</span>
        {usagePercent !== undefined ? <span className="context-meter" title={`估算上下文：${contextUsage?.estimatedTokens.toLocaleString()} / ${contextUsage?.contextWindowTokens.toLocaleString()} tokens；不是计费数据`}><meter min={0} max={100} value={usagePercent} aria-label="估算上下文使用比例" />{usagePercent}%</span> : null}
      </div>
    </footer>
  );
}
