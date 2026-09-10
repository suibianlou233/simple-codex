import { useEffect, useState } from "react";
import { mediaBridge, type BrowserPending, type BrowserScope } from "../bridge/mediaBridge";

const actions: Record<string, string> = {open:"打开网页",read:"读取网页",click:"点击元素",fill:"填写内容",scroll:"滚动页面",screenshot:"查看截图",back:"访问上一页",forward:"访问下一页",reload:"刷新网页"};
export function BrowserApproval({ taskNames, onVisibilityChange }: { taskNames: Record<string, string>; onVisibilityChange?: (visible: boolean) => void }) {
  const [pending, setPending] = useState<BrowserPending[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => { onVisibilityChange?.(pending.length > 0); return () => onVisibilityChange?.(false); }, [pending.length, onVisibilityChange]);
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try { const items = await mediaBridge.browserPending(); if (!stopped) setPending(items); }
      catch { if (!stopped) setPending([]); }
      finally { if (!stopped) timer = setTimeout(() => void poll(), 750); }
    };
    void poll();
    return () => { stopped = true; clearTimeout(timer); };
  }, []);
  const item = pending[0];
  if (!item) return null;
  const resolve = async (scope: BrowserScope) => {
    setBusy(true); setError("");
    try { await mediaBridge.browserResolve(item.id, scope); setPending(all => all.filter(p => p.id !== item.id)); }
    catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  };
  return <aside className="browser-approval" role="dialog" aria-label="浏览器访问确认" aria-describedby="browser-approval-detail">
    <strong>允许 AI 操作这个网站？</strong>
    <p>{taskNames[item.taskId] ?? "后台任务"} · {actions[item.action] ?? item.action}</p>
    <code>{item.origin}</code>
    <details><summary>操作详情</summary><p>{item.url}</p>{item.selector ? <code>{item.selector}</code> : null}{item.text ? <pre>{item.text}</pre> : null}</details>
    <p id="browser-approval-detail">网页文字和截图可能发送到当前模型。回合或会话授权允许 AI 在该网站继续读取、填写和点击；可在浏览器权限中随时撤销。</p>
    <div>{([['deny','拒绝'],['once','仅本次'],['turn','当前回合'],['thread','当前会话']] as const).map(([scope,label]) => <button key={scope} type="button" disabled={busy} onClick={() => void resolve(scope)}>{label}</button>)}</div>
    {error ? <p role="alert">{error}</p> : null}
  </aside>;
}
