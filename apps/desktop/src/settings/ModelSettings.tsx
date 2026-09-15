import { useState } from "react";
import { useDialog } from "../components/useDialog";
import type { ContextUsage, ModelProfileSummary, SaveModelProfileInput } from "../bridge/types";

const MIN_CONTEXT_WINDOW_TOKENS = 16_384;
const MIN_OUTPUT_TOKENS = 1_024;
const MIN_INPUT_BUDGET_TOKENS = 8_192;

export function ModelSettings({
  activeProfile,
  contextUsage,
  disabled,
  onClose,
  onSave,
  onShowGuide,
  onManageSkills,
}: {
  activeProfile?: ModelProfileSummary;
  contextUsage?: ContextUsage;
  disabled: boolean;
  onClose: () => void;
  onSave: (input: SaveModelProfileInput) => Promise<void>;
  onShowGuide?: () => void;
  onManageSkills?: () => void;
}) {
  const dialogRef = useDialog<HTMLFormElement>(onClose);
  const [dialect, setDialect] = useState<SaveModelProfileInput["dialect"]>(
    activeProfile?.dialect ?? "deep_seek",
  );
  const [name, setName] = useState(activeProfile?.name ?? "DeepSeek");
  const [baseUrl, setBaseUrl] = useState(
    activeProfile?.baseUrl ?? "https://api.deepseek.com",
  );
  const [model, setModel] = useState(activeProfile?.model ?? "deepseek-v4-flash");
  const [apiKey, setApiKey] = useState("");
  const [newProfile, setNewProfile] = useState(false);
  const [timeoutSeconds, setTimeoutSeconds] = useState((activeProfile?.timeoutMs ?? 300000) / 1000);
  const isTokenPlan = baseUrl.includes("token-plan.");
  const [contextWindowTokens, setContextWindowTokens] = useState(
    activeProfile?.contextWindowTokens ?? 1_048_576,
  );
  const [maxOutputTokens, setMaxOutputTokens] = useState(
    activeProfile?.maxOutputTokens ?? 32_768,
  );
  const modelTokenBudgetError =
    contextWindowTokens < MIN_CONTEXT_WINDOW_TOKENS
      ? `上下文窗口不能小于 ${MIN_CONTEXT_WINDOW_TOKENS.toLocaleString()} Token`
      : maxOutputTokens < MIN_OUTPUT_TOKENS
        ? `单次输出上限不能小于 ${MIN_OUTPUT_TOKENS.toLocaleString()} Token`
        : maxOutputTokens + MIN_INPUT_BUDGET_TOKENS > contextWindowTokens
          ? `至少为 Agent 输入保留 ${MIN_INPUT_BUDGET_TOKENS.toLocaleString()} Token；请降低输出上限或提高上下文窗口`
          : undefined;

  const applyPreset = (value: SaveModelProfileInput["dialect"] | "qwen_token_plan") => {
    setDialect(value === "qwen_token_plan" ? "qwen" : value);
    if (value === "qwen_token_plan") {
      setNewProfile(!activeProfile?.baseUrl.includes("token-plan."));
      setApiKey("");
      setName("千问 Token Plan");
      setBaseUrl("https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1");
      setModel("qwen3.8-max");
      setContextWindowTokens(1_000_000);
      setMaxOutputTokens(32_768);
      setTimeoutSeconds(300);
    } else if (value === "deep_seek") {
      setName("DeepSeek");
      setBaseUrl("https://api.deepseek.com");
      setModel("deepseek-v4-flash");
      setContextWindowTokens(1_048_576);
      setMaxOutputTokens(32_768);
    } else if (value === "qwen") {
      setName("通义千问");
      setBaseUrl("https://dashscope.aliyuncs.com/compatible-mode/v1");
      setModel("qwen-plus");
      setContextWindowTokens(1_000_000);
      setMaxOutputTokens(32_768);
    } else {
      setName("OpenAI 兼容模型");
      setBaseUrl("http://127.0.0.1:8000/v1");
      setModel("");
      setContextWindowTokens(131_072);
      setMaxOutputTokens(16_384);
    }
  };

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <form
        ref={dialogRef}
        tabIndex={-1}
        className="model-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="model-settings-title"
        onMouseDown={(event) => event.stopPropagation()}
        onSubmit={(event) => {
          event.preventDefault();
          if (modelTokenBudgetError) return;
          void onSave({
            profileId: newProfile ? undefined : activeProfile?.id,
            name,
            baseUrl,
            model,
            dialect,
            apiKey: apiKey || undefined,
            maxOutputTokens,
            contextWindowTokens,
            timeoutMs: Math.round(timeoutSeconds * 1000),
            isDefault: true,
          });
        }}
      >
        <div className="create-heading">
          <div>
            <p className="eyebrow">本地模型设置</p>
            <h2 id="model-settings-title">{activeProfile ? "管理模型" : "连接你的模型"}</h2>
          </div>
          <button className="text-button" type="button" onClick={onClose}>关闭</button>
        </div>
        <div className="preset-row">
          {(["deep_seek", "qwen", "qwen_token_plan", "standard"] as const).map((value) => (
            <button
              className={(value === "qwen_token_plan" ? isTokenPlan : dialect === value && !isTokenPlan) ? "is-active" : undefined}
              type="button"
              key={value}
              onClick={() => applyPreset(value)}
            >
              {value === "deep_seek" ? "DeepSeek" : value === "qwen" ? "通义千问" : value === "qwen_token_plan" ? "千问 Token Plan" : "兼容接口"}
            </button>
          ))}
        </div>
        <label><span>显示名称</span><input value={name} onChange={(event) => setName(event.target.value)} required /></label>
        <label><span>接口地址</span><input value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} required /></label>
        <label><span>模型名称</span><input value={model} onChange={(event) => setModel(event.target.value)} required /></label>
        {isTokenPlan ? <p className="settings-help">使用北京地域 Token Plan 专属 API Key。默认 qwen3.8-max，支持直接读取上传图片和工具截图；qwen3.8-flash 也可读图。其他模型请按套餐支持情况填写。{newProfile ? "保存后新增配置，保留原有模型。" : ""}</p> : null}
        <label><span>无响应等待时间（秒）</span><input type="number" min={1} max={600} step={1} value={timeoutSeconds} onChange={event=>setTimeoutSeconds(event.currentTarget.valueAsNumber)} required /></label>
        <p className="settings-help">只在连续未收到模型数据时超时；持续输出会重新计时，不限制一次回答的总时长。</p>
        {dialect === "deep_seek" ? <p className="settings-help">发送图片或浏览器截图时，会自动使用 deepseek-v4-flash-vision-exp，无需手动切换。上下文仍含图片时继续使用视觉模型；不含图片时使用上方所选模型。沿用当前接口和密钥，视觉模型按供应商规则计费。</p> : null}
        <label>
          <span>上下文窗口（Token）</span>
          <input
            type="number"
            name="contextWindowTokens"
            min={MIN_CONTEXT_WINDOW_TOKENS}
            max={1048576}
            step={1}
            value={contextWindowTokens}
            onChange={(event) => {
              const next = event.currentTarget.valueAsNumber;
              if (Number.isFinite(next)) setContextWindowTokens(next);
            }}
            required
          />
        </label>
        <label>
          <span>单次输出上限（Token）</span>
          <input
            type="number"
            name="maxOutputTokens"
            min={MIN_OUTPUT_TOKENS}
            max={Math.max(MIN_OUTPUT_TOKENS, contextWindowTokens - MIN_INPUT_BUDGET_TOKENS)}
            step={1024}
            value={maxOutputTokens}
            onChange={(event) => {
              const next = event.currentTarget.valueAsNumber;
              if (Number.isFinite(next)) setMaxOutputTokens(next);
            }}
            required
          />
        </label>
        {modelTokenBudgetError ? (
          <div className="model-budget-error" role="alert">{modelTokenBudgetError}</div>
        ) : null}
        <label>
          <span>API Key</span>
          <input
            type="password"
            autoComplete="off"
            value={apiKey}
            placeholder={!newProfile && activeProfile?.hasCredential ? "已安全保存；留空保持不变" : "只保存到系统凭据库"}
            onChange={(event) => setApiKey(event.target.value)}
            required={dialect !== "standard" && (newProfile || !activeProfile?.hasCredential)}
          />
        </label>
        <div className="thinking-lock">✓ DeepSeek 与千问始终关闭思考，优先首字速度</div>
        {contextUsage ? (
          <section className="context-usage" aria-label="当前对话上下文用量">
            <div>
              <strong>当前对话上下文</strong>
              <span>
                约 {contextUsage.estimatedTokens.toLocaleString()} / {contextUsage.contextWindowTokens.toLocaleString()} Token
              </span>
            </div>
            <progress
              max={Math.max(1, contextUsage.contextWindowTokens - contextUsage.reservedOutputTokens)}
              value={Math.min(
                contextUsage.estimatedTokens,
                Math.max(1, contextUsage.contextWindowTokens - contextUsage.reservedOutputTokens),
              )}
            />
            <small>
              {contextUsage.messageCount} 条消息 · {contextUsage.toolExchangeCount} 次工具上下文 · 已预留 {contextUsage.reservedOutputTokens.toLocaleString()} Token 输出
            </small>
          </section>
        ) : null}
        <button
          className="primary-action save-model"
          type="submit"
          disabled={disabled || Boolean(modelTokenBudgetError)}
        >
          保存并使用
          <span aria-hidden="true">→</span>
        </button>
        {onManageSkills && <button className="text-button" type="button" onClick={onManageSkills}>技能管理</button>}
        {onShowGuide ? <button className="text-button" type="button" disabled={disabled} onClick={onShowGuide}>查看使用引导</button> : null}
      </form>
    </div>
  );
}
