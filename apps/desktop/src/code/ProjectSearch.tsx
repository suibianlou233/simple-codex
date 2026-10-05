import {useEffect,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
export type Location={path:string;line:number;column?:number};
export function ProjectSearch({projectId,onOpen}:{projectId:string;onOpen:(location:Location)=>void}) {
 const [query,setQuery]=useState(""),[sensitive,setSensitive]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState("");
 const [result,setResult]=useState<{hits:(Location&{text:string})[];limited:boolean;scanned:number}>();
 useEffect(()=>{let alive=true;setResult(undefined);setError("");if(!query){setBusy(false);return;}setBusy(true);const timer=setTimeout(()=>{void invoke<typeof result>("editor_grep",{projectId,query,caseSensitive:sensitive}).then(r=>{if(alive)setResult(r);}).catch(e=>{if(alive)setError(String(e));}).finally(()=>{if(alive)setBusy(false);});},300);return()=>{alive=false;clearTimeout(timer);};},[projectId,query,sensitive]);
 return <section className="project-search"><input aria-label="搜索项目正文" placeholder="搜索项目正文…" value={query} onChange={e=>setQuery(e.target.value)}/><label><input type="checkbox" checked={sensitive} onChange={e=>setSensitive(e.target.checked)}/>区分大小写</label>{busy?<small>搜索中…</small>:null}{error?<p role="alert">{error}</p>:null}{result?<><small>{result.hits.length} 个匹配 · 已扫描 {result.scanned} 个文本文件{result.limited?" · 已达检索上限，请缩小关键词":""}</small>{result.hits.map((hit,i)=><button key={i} title={hit.text} onClick={()=>onOpen(hit)}><strong>{hit.path}:{hit.line}</strong><span>{hit.text}</span></button>)}</>:null}<small>搜索磁盘内容；遵循项目文件与敏感文件过滤。</small></section>;
}
