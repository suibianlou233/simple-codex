import { useEffect, useRef, useState } from "react";
import type { PermissionLevel } from "../bridge/types";

const permissionOptions: Array<{
  value: PermissionLevel;
  level: string;
  title: string;
  summary: string;
}> = [
  {
    value: "approval",
    level: "1",
    title: "逐项确认",
    summary: "修改文件或运行命令前询问你",
  },
  {
    value: "project_full_access",
    level: "2",
    title: "项目自动",
    summary: "仅在当前项目文件夹内自动操作",
  },
  {
    value: "system_full_access",
    level: "3",
    title: "全机自动",
    summary: "可操作这台电脑，不再逐项询问",
  },
];

const permissionLabels: Record<PermissionLevel, string> = {
  approval: "逐项确认",
  project_full_access: "项目自动",
  system_full_access: "全机自动",
};

const permissionWarnings: Record<Exclude<PermissionLevel, "approval">, string> = {
  project_full_access: "项目内写入和命令不再逐项询问。Windows 原生命令使用受限令牌限制写入，允许联网；不隔离项目外读取，也不是完整系统隔离。",
  system_full_access: "可操作整台电脑，文件和命令操作不再逐项询问。",
};

export function PermissionSelector({
  value,
  disabled,
  disabledMessage,
  defaultOpen = false,
  scopeLabel = "只影响当前对话",
  workspaceSandboxReady,
  onInstallSandbox,
  onChange,
}: {
  value: PermissionLevel;
  disabled: boolean;
  disabledMessage?: string;
  defaultOpen?: boolean;
  scopeLabel?: string;
  workspaceSandboxReady: boolean;
  onInstallSandbox?: () => Promise<boolean>;
  onChange: (permissionLevel: PermissionLevel) => Promise<boolean>;
}) {
  const [pending, setPending] = useState<PermissionLevel>();
  const [isChanging, setIsChanging] = useState(false);
  const [isOpen, setIsOpen] = useState(defaultOpen);
  const controlRef = useRef<HTMLDivElement>(null);
  const current = permissionOptions.find((option) => option.value === value) ?? permissionOptions[0];

  useEffect(() => {
    if (!isOpen) return undefined;
    const closeOnPointerDown = (event: PointerEvent) => {
      if (!controlRef.current?.contains(event.target as Node)) {
        setPending(undefined);
        setIsOpen(false);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setPending(undefined);
        setIsOpen(false);
      }
    };
    document.addEventListener("pointerdown", closeOnPointerDown);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnPointerDown);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [isOpen]);

  const apply = async (next: PermissionLevel) => {
    setIsChanging(true);
    try {
      const changed = await onChange(next);
      if (changed) {
        setPending(undefined);
        setIsOpen(false);
      }
    } finally {
      setIsChanging(false);
    }
  };

  return (
    <div ref={controlRef} className={`permission-control permission-${value}${isOpen ? " is-open" : ""}`}>
      <button
        className="permission-trigger"
        type="button"
        aria-label={`任务权限：${current.title}`}
        aria-expanded={isOpen}
        aria-haspopup="dialog"
        disabled={disabled || isChanging}
        onClick={() => {
          setPending(undefined);
          setIsOpen((open) => !open);
        }}
      >
        <span className="permission-level-mark" aria-hidden="true">{current.level}</span>
        <span className="permission-trigger-copy">
          <strong>{current.title}</strong>
          <small>{current.summary}</small>
        </span>
        <span className="permission-chevron" aria-hidden="true" />
      </button>
      {disabled && disabledMessage ? <span className="permission-lock">{disabledMessage}</span> : null}
      {isOpen ? (
        <div className="permission-popover" role="dialog" aria-label="选择任务权限">
          <div className="permission-popover-heading">
            <strong>任务权限</strong>
            <span>{scopeLabel}</span>
          </div>
          <div className="permission-options" role="radiogroup" aria-label="权限级别">
            {permissionOptions.map((option) => {
              const unavailable = option.value === "project_full_access" && !workspaceSandboxReady;
              const selected = option.value === value;
              return (
                <button
                  key={option.value}
                  className={`permission-option${selected ? " is-selected" : ""}`}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  disabled={unavailable || isChanging}
                  onClick={() => {
                    if (selected) {
                      setIsOpen(false);
                    } else if (option.value === "approval") {
                      setPending(undefined);
                      void apply(option.value);
                    } else {
                      setPending(option.value);
                    }
                  }}
                >
                  <span className="permission-level-mark" aria-hidden="true">{option.level}</span>
                  <span>
                    <strong>{option.title}</strong>
                    <small>{option.summary}</small>
                  </span>
                  {unavailable ? <em>沙箱未安装</em> : selected ? <em>当前</em> : null}
                </button>
              );
            })}
          </div>
          {!workspaceSandboxReady ? (
            <div className="permission-availability" role="status">
              <span>二级权限需要先安装本机项目沙箱。</span>
              {onInstallSandbox ? (
                <button
                  type="button"
                  disabled={isChanging}
                  onClick={async () => {
                    setIsChanging(true);
                    try {
                      await onInstallSandbox();
                    } finally {
                      setIsChanging(false);
                    }
                  }}
                >
                  {isChanging ? "正在安装…" : "安装沙箱"}
                </button>
              ) : null}
            </div>
          ) : null}
          {pending && pending !== "approval" ? (
            <div className={`permission-confirm permission-confirm-${pending}`} role="alert">
              <span>
                <strong>开启{permissionLabels[pending]}？</strong>
                {permissionWarnings[pending]}
              </span>
              <div>
                <button type="button" disabled={isChanging} onClick={() => setPending(undefined)}>
                  取消
                </button>
                <button type="button" disabled={isChanging} onClick={() => void apply(pending)}>
                  确认开启
                </button>
              </div>
            </div>
          ) : null}
          <button
            className="permission-popover-close"
            type="button"
            aria-label="关闭权限选择"
            onClick={() => {
              setPending(undefined);
              setIsOpen(false);
            }}
          >
            ×
          </button>
        </div>
      ) : null}
    </div>
  );
}
