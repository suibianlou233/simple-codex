import { useEffect, useRef, useState } from "react";
import { mediaBridge, type BrowserAction, type BrowserPolicy } from "../bridge/mediaBridge";
import { Icon } from "../components/Icon";
import type { AttachmentSummary } from "../bridge/types";

export function BrowserPanel({ projectId, onAttach, onClose, suspended = false }: { projectId: string; onAttach: (attachment: AttachmentSummary) => void; onClose: () => void; suspended?: boolean }) {
  const [url, setUrl] = useState("http://localhost:3000");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [policy, setPolicy] = useState<BrowserPolicy>();
  const [deniedSites, setDeniedSites] = useState("");
  const editingAddress = useRef(false);
  const viewport = useRef<HTMLDivElement>(null);
  const [policyOpen, setPolicyOpen] = useState(false);
  const [hasPage, setHasPage] = useState(false);
  useEffect(() => {
    const element = viewport.current;
    if (!element) return;
    let stopped = false;
    let frame = 0;
    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (stopped) return;
        const rect = element.getBoundingClientRect();
        const scale = window.devicePixelRatio || 1;
        const bounds = suspended || policyOpen || rect.width < 1 || rect.height < 1 ? null : {x:rect.x*scale,y:rect.y*scale,width:rect.width*scale,height:rect.height*scale};
        void mediaBridge.browserViewport(projectId,bounds).catch(error => {if (!stopped) setMessage(String(error));});
      });
    };
    const observer = new ResizeObserver(update);
    observer.observe(element); window.addEventListener("resize",update); update();
    return () => {stopped=true;cancelAnimationFrame(frame);observer.disconnect();window.removeEventListener("resize",update);void mediaBridge.browserViewport(projectId,null).catch(()=>undefined);};
  }, [projectId,suspended,policyOpen]);
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try { const current = await mediaBridge.browserStatus(projectId); if (!stopped) { setHasPage(Boolean(current)); if(current && !editingAddress.current) setUrl(current); } }
      catch { /* Explicit operations surface connection errors. */ }
      finally { if (!stopped) timer = setTimeout(() => void poll(), 1500); }
    };
    void poll();
    return () => { stopped = true; clearTimeout(timer); };
  }, [projectId]);
  useEffect(() => {
    let cancelled = false;
    void mediaBridge.browserPolicy(projectId).then(result => {
      if (!cancelled) { setPolicy(result.policy); setDeniedSites(result.policy.deniedOrigins.join("\n")); }
    }).catch(error => { if (!cancelled) setMessage(String(error)); });
    return () => { cancelled = true; };
  }, [projectId]);
  const savePolicy = async (revoke = false) => {
    setBusy(true);
    try {
      const result = await mediaBridge.browserPolicy(projectId, revoke ? undefined : policy && {...policy, deniedOrigins:deniedSites.split(/\n/).map(s=>s.trim()).filter(Boolean)}, revoke);
      setPolicy(result.policy); setDeniedSites(result.policy.deniedOrigins.join("\n"));
      setMessage(revoke ? "已撤销当前项目的全部网站授权和待确认请求。" : "权限已保存，原有授权已撤销。");
    } catch(error) { setMessage(String(error)); }
    finally { setBusy(false); }
  };
  const run = async (request: BrowserAction) => {
    setBusy(true); setMessage("");
    try {
      const result = await mediaBridge.browser(projectId, request);
      if (result.url) { setUrl(result.url); setHasPage(true); }
      if (result.attachment) { onAttach(result.attachment); setMessage("截图已添加到输入框，发送后模型才能看到。"); }
      else if (request.action === "close") { setHasPage(false); setMessage("网页已关闭。"); }
      else setMessage("网页已在右侧打开。");
    } catch (error) { setMessage(typeof error === "string" ? error : error instanceof Error ? error.message : "浏览器操作失败"); }
    finally { setBusy(false); }
  };
  return <section className="browser-panel" aria-label="内置浏览器">
    <header><strong><Icon name="globe" size={16} />浏览器</strong><div><button type="button" disabled={busy || !hasPage} onClick={() => void run({ action: "screenshot" })} title="截图添加到输入框">截图</button><button className="icon-button" type="button" onClick={onClose} aria-label="收起浏览器"><Icon name="close" size={16} /></button></div></header>
    <form onSubmit={event => { event.preventDefault(); void run({ action: "open", url }); }}>
      <button type="button" disabled={busy} onClick={() => void run({ action: "back" })} aria-label="网页后退">←</button>
      <button type="button" disabled={busy} onClick={() => void run({ action: "forward" })} aria-label="网页前进">→</button>
      <button type="button" disabled={busy} onClick={() => void run({ action: "reload" })}>刷新</button>
      <input aria-label="网页地址" value={url} onFocus={() => { editingAddress.current = true; }} onBlur={() => { editingAddress.current = false; }} onChange={event => setUrl(event.target.value)} placeholder="http://localhost:3000" />
      <button type="submit" disabled={busy}>打开网页</button>
    </form>
    <p role="status">{busy ? "正在处理…" : message || "网页数据按项目保存在本机。AI 首次访问网站时会请求授权，可选择本次、回合或会话范围。"}</p>
    <details className="browser-policy" onToggle={event=>setPolicyOpen(event.currentTarget.open)}><summary>网站权限</summary>
      {policy ? <>
        <label><input type="checkbox" checked={policy.denyAll} onChange={e=>setPolicy({...policy,denyAll:e.target.checked})} />禁止此项目的 AI 浏览器访问</label>
        <label><input type="checkbox" checked={policy.allowHistoryAccess} onChange={e=>setPolicy({...policy,allowHistoryAccess:e.target.checked})} />允许 AI 使用前进与后退</label>
        <label>禁止访问的网站（每行一个完整地址）<textarea value={deniedSites} onChange={e=>setDeniedSites(e.target.value)} placeholder="https://example.com" /></label>
        <p>上传、下载、完整浏览器调试访问均禁用。授权不会跨项目、跨会话或在重启后保留；网站规则保存在本机。</p>
        <button type="button" disabled={busy} onClick={()=>void savePolicy()}>保存权限</button>
        <button type="button" disabled={busy} onClick={()=>void savePolicy(true)}>撤销全部网站授权</button>
      </> : <p>正在读取权限…</p>}
    </details>
    <div ref={viewport} className="browser-viewport" aria-label="网页显示区域">
      <div className="browser-placeholder"><Icon name="search" size={28} /><strong>{suspended || policyOpen ? "网页暂时隐藏" : hasPage ? "正在显示网页…" : "在这里预览网页"}</strong><p>{hasPage ? "网页将在关闭弹层后恢复。" : "输入网址，或让 Simple 打开项目页面。"}</p></div>
    </div>
    <footer><span>本地浏览器 · 按网站授权</span><button type="button" disabled={busy || !hasPage} onClick={()=>void run({action:"close"})}>关闭网页</button></footer>
  </section>;
}
