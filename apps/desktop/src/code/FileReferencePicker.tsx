import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useDialog } from "../components/useDialog";

export function FileReferencePicker({projectId,onPick,onClose}:{projectId:string;onPick:(path:string)=>Promise<void>;onClose:()=>void}) {
  const ref=useDialog<HTMLElement>(onClose);
  const [query,setQuery]=useState(""), [paths,setPaths]=useState<string[]>([]), [error,setError]=useState("");
  const [busy,setBusy]=useState(false);
  useEffect(()=>{let active=true;const timer=setTimeout(()=>{void invoke<string[]>("editor_search",{projectId,query}).then(result=>{if(active)setPaths(result);}).catch(e=>{if(active)setError(String(e));});},150);return()=>{active=false;clearTimeout(timer);};},[projectId,query]);
  return <div className="modal-backdrop"><section ref={ref} className="code-reference-picker" role="dialog" aria-modal="true" aria-label="引用项目文件" tabIndex={-1}><header><strong>引用项目文件</strong><button onClick={onClose}>关闭</button></header><input autoFocus aria-label="搜索引用文件" placeholder="输入文件名或路径…" value={query} onChange={e=>setQuery(e.target.value)}/><small>添加内容快照；已打开的文件优先使用编辑器内容。最多展示 100 项。</small>{error?<p role="alert">{error}</p>:null}<div>{paths.map(path=><button disabled={busy} key={path} onClick={()=>{setBusy(true);void onPick(path).then(onClose).catch(e=>setError(String(e))).finally(()=>setBusy(false));}}>{path}</button>)}</div></section></div>;
}
