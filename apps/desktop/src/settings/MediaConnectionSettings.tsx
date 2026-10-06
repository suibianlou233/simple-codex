import { useEffect, useState } from "react";
import { mediaGenerationBridge } from "../bridge/mediaGenerationBridge";
import { useDialog } from "../components/useDialog";

const IMAGE_MODELS = [
  ["doubao-seedream-5-0-flash-260915", "Seedream 5.0 Flash · 推荐"],
  ["doubao-seedream-5-0-pro-260628", "Seedream 5.0 Pro"],
  ["doubao-seedream-5-0-260128", "Seedream 5.0 Lite"],
] as const;

const VIDEO_MODELS = [
  ["doubao-seedance-2-0-mini-260615", "Seedance 2.0 Mini · 低成本"],
  ["doubao-seedance-2-0-fast-260128", "Seedance 2.0 Fast"],
  ["doubao-seedance-2-0-260128", "Seedance 2.0"],
  ["doubao-seedance-2-5-260628", "Seedance 2.5 · 高质量"],
] as const;

export function MediaConnectionSettings({ onClose }: { onClose: () => void }) {
  const dialogRef = useDialog<HTMLFormElement>(onClose);
  const [baseUrl, setBaseUrl] = useState("https://ark.cn-beijing.volces.com/api/v3");
  const [imageModel, setImageModel] = useState<string>(IMAGE_MODELS[0][0]);
  const [videoModel, setVideoModel] = useState<string>(VIDEO_MODELS[0][0]);
  const [hasCredential, setHasCredential] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string>();

  useEffect(() => {
    let current = true;
    void mediaGenerationBridge.settings().then(settings => {
      if (!current) return;
      setBaseUrl(settings.baseUrl);
      setImageModel(settings.imageModel);
      setVideoModel(settings.videoModel);
      setHasCredential(settings.hasCredential);
    }).catch(reason => current && setError(String(reason))).finally(() => current && setBusy(false));
    return () => { current = false; };
  }, []);

  return <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
    <form ref={dialogRef} tabIndex={-1} className="model-modal" role="dialog" aria-modal="true" aria-labelledby="media-settings-title"
      onMouseDown={event => event.stopPropagation()} onSubmit={event => {
        event.preventDefault();
        setBusy(true); setError(undefined);
        void mediaGenerationBridge.saveSettings({ baseUrl, imageModel, videoModel, apiKey: apiKey || undefined })
          .then(settings => { setHasCredential(settings.hasCredential); setApiKey(""); onClose(); })
          .catch(reason => setError(String(reason))).finally(() => setBusy(false));
      }}>
      <div className="create-heading"><div><p className="eyebrow">对话内生成</p><h2 id="media-settings-title">图片与视频设置</h2></div><button className="text-button" type="button" onClick={onClose}>关闭</button></div>
      <p className="settings-help">配置一次后，直接在普通对话中说“画一张……”或“生成一段视频……”。无需打开单独模式。</p>
      <label><span>火山方舟接口地址</span><input value={baseUrl} onChange={event => setBaseUrl(event.target.value)} required /></label>
      <label><span>默认图片模型</span><select value={imageModel} onChange={event => setImageModel(event.target.value)}>{IMAGE_MODELS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
      <label><span>默认视频模型</span><select value={videoModel} onChange={event => setVideoModel(event.target.value)}>{VIDEO_MODELS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
      <label><span>API Key</span><input type="password" autoComplete="off" value={apiKey} onChange={event => setApiKey(event.target.value)} required={!hasCredential} placeholder={hasCredential ? "已安全保存；留空保持不变" : "只保存到系统凭据库"} /></label>
      <p className="settings-help">密钥不会写入项目、技能、对话或日志。生成请求会按火山方舟账单计费。</p>
      {error ? <div className="model-budget-error" role="alert">{error}</div> : null}
      <button className="primary-action save-model" type="submit" disabled={busy}>{busy ? "正在保存…" : "保存图片与视频设置"}<span aria-hidden="true">→</span></button>
    </form>
  </div>;
}
