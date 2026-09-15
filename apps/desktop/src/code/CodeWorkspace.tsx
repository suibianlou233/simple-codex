import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CodeEditor } from "./CodeEditor";
import { reconcile, type Buffer, type EditorFile } from "./editorModel";
import type { ProjectSummary } from "../bridge/types";
type Entry = {path:string; name:string; directory:boolean};
const key = (project:string,path:string)=>`${project}\0${path}`;
export function CodeWorkspace({project,visible,dark,onOpenProject,onAttach,request,onModalVisibility}:{project?:ProjectSummary;visible:boolean;dark:boolean;onOpenProject:()=>void;onAttach:(text:string)=>void;request?:{path:string;nonce:number};onModalVisibility:(open:boolean)=>void}) {
  const [buffers,setBuffers]=useState<Record<string,Buffer>>({});
  const [active,setActive]=useState<Record<string,string>>({});
  const [tree,setTree]=useState<Record<string,Entry[]>>({});
  const [expanded,setExpanded]=useState<Set<string>>(new Set());
  const [query,setQuery]=useState(""); const [results,setResults]=useState<string[]>([]);
  const [error,setError]=useState(""); const [busy,setBusy]=useState(false);
  const [form,setForm]=useState<{kind:"file"|"folder"|"rename";path:string}>(); const [name,setName]=useState("");
  const [selected,setSelected]=useState(""); const [compare,setCompare]=useState(false);
  const [closing,setClosing]=useState<string>(); const [closeWindow,setCloseWindow]=useState(false);
  useEffect(()=>{onModalVisibility(closeWindow);return()=>onModalVisibility(false);},[closeWindow,onModalVisibility]);
  const [reload,setReload]=useState(false);
  const state=useRef(buffers);state.current=buffers;
  const projectId=project?.id;
  const activeKey=projectId?active[projectId]:undefined; const file=activeKey?buffers[activeKey]:undefined;
  const dirty=Object.values(buffers).some(f=>f.content!==f.saved);
  const dirtyRef=useRef(dirty);dirtyRef.current=dirty;
  useEffect(()=>{
    const before=(event:BeforeUnloadEvent)=>{if(dirtyRef.current){event.preventDefault();event.returnValue="";}};
    window.addEventListener("beforeunload",before);
    let disposed=false; let unlisten:(()=>void)|undefined;
    if("__TAURI_INTERNALS__" in window) void getCurrentWindow().onCloseRequested(event=>{if(dirtyRef.current){event.preventDefault();setCloseWindow(true);}}).then(stop=>{if(disposed)stop();else unlisten=stop;});
    return ()=>{disposed=true;unlisten?.();window.removeEventListener("beforeunload",before);};
  },[]);
  async function list(path="") {if(!projectId)return; const entries=await invoke<Entry[]>("editor_list",{projectId,path});setTree(t=>({...t,[key(projectId,path)]:entries}));}
  async function run(action:()=>Promise<void>) {setBusy(true);setError("");try{await action();}catch(e){setError(String(e));}finally{setBusy(false);}}
  async function open(path:string) {
    if(!projectId)return; const id=key(projectId,path);
    if(!state.current[id]) {const data=await invoke<EditorFile>("editor_read",{projectId,path});setBuffers(all=>all[id]?all:{...all,[id]:{...data,projectId,saved:data.content}});}
    setActive(all=>({...all,[projectId]:id}));setSelected("");setCompare(false);setReload(false);
  }
  useEffect(()=>{setQuery("");setForm(undefined);setError("");if(projectId&&visible)void run(()=>list());},[projectId,visible]);
  useEffect(()=>{if(request&&projectId)void run(()=>open(request.path));},[request,projectId]);
  useEffect(()=>{if(!projectId||!query.trim()){setResults([]);return;}let cancelled=false;
    const timer=setTimeout(()=>{void invoke<string[]>("editor_search",{projectId,query}).then(paths=>{if(!cancelled)setResults(paths);}).catch(e=>{if(!cancelled)setError(String(e));});},180);
    return()=>{cancelled=true;clearTimeout(timer);};},[query,projectId]);
  useEffect(()=>{if(!file||!activeKey||!visible)return;let disposed=false;const id=activeKey;
    let reading=false;
    const check=()=>{if(reading)return;reading=true;const version=state.current[id]?.sha256;void invoke<EditorFile>("editor_read",{projectId:file.projectId,path:file.path}).then(disk=>{if(!disposed)setBuffers(all=>all[id]&&all[id].sha256===version?{...all,[id]:reconcile(all[id],disk)}:all);}).catch(e=>{if(!disposed)setError(`无法检查当前文件：${String(e)}`);}).finally(()=>{reading=false;});};
    const timer=setInterval(check,4000);window.addEventListener("focus",check);check();return()=>{disposed=true;clearInterval(timer);window.removeEventListener("focus",check);};
  },[activeKey,visible]);
  async function save(){if(!file||!activeKey||busy||file.disk)return;const id=activeKey;const submitted=file.content;
    await run(async()=>{const data=await invoke<EditorFile>("editor_save",{projectId:file.projectId,path:file.path,content:submitted,expected:file.sha256});setBuffers(all=>all[id]?({...all,[id]:{...all[id],sha256:data.sha256,saved:data.content,disk:undefined}}):all);});
  }
  function remove(id:string){setBuffers(all=>{const next={...all};delete next[id];return next;});setClosing(undefined);}
  async function mutate(){if(!projectId||!form)return;const operation=form;await run(async()=>{
    if(operation.kind==="rename"){
      const affected=Object.values(state.current).filter(f=>f.projectId===projectId&&(f.path===operation.path||f.path.startsWith(operation.path+"/")));
      if(affected.some(f=>f.content!==f.saved))throw new Error("请先保存这个文件或目录中的修改，再重命名");
      await invoke("editor_rename",{projectId,path:operation.path,destination:name});
      setBuffers(all=>{const next={...all};for(const f of affected){delete next[key(projectId,f.path)];}return next;});
      setActive(all=>({...all,[projectId]:""}));
    }else await invoke("editor_create",{projectId,path:name,directory:operation.kind==="folder"});
    setTree({});setExpanded(new Set());await list();setForm(undefined);
    if(operation.kind==="file")await open(name);
  });}
  function node(entry:Entry,depth:number):React.ReactNode{
    const id=key(projectId!,entry.path);const isOpen=expanded.has(id);
    return <div key={id}><div className="code-tree-row" style={{paddingLeft:8+depth*12}}>
      <button className={file?.path===entry.path?"is-active":""} title={entry.path} onClick={()=>void run(async()=>{if(entry.directory){if(!isOpen)await list(entry.path);setExpanded(previous=>{const next=new Set(previous);if(isOpen)next.delete(id);else next.add(id);return next;});}else await open(entry.path);})}>{entry.directory?(isOpen?"▾ ▣ ":"▸ ▣ "):"  · "}{entry.name}</button>
      <button className="code-rename" aria-label={`重命名 ${entry.name}`} title="重命名" onClick={()=>{setForm({kind:"rename",path:entry.path});setName(entry.path);}}>···</button>
    </div>{entry.directory&&isOpen?(tree[id]??[]).map(item=>node(item,depth+1)):null}</div>;
  }
  return <section className="code-workspace" hidden={!visible} aria-label="Simple Code 编辑区">
    {!project?<div className="code-empty"><h2>Simple Code</h2><p>打开一个项目，开始阅读和修改代码。</p><button onClick={onOpenProject}>打开文件夹</button></div>:<>
    <aside className="code-files"><header><strong>文件</strong><button title="刷新文件树" onClick={()=>void run(async()=>{setTree({});setExpanded(new Set());await list();})}>↻</button><button title="新建文件" onClick={()=>{setForm({kind:"file",path:""});setName("");}}>＋</button><button title="新建文件夹" onClick={()=>{setForm({kind:"folder",path:""});setName("");}}>▣＋</button></header>
      <input aria-label="搜索项目文件" placeholder="查找文件…" value={query} onChange={e=>setQuery(e.target.value)}/>
      {form?<form className="code-path-form" onSubmit={e=>{e.preventDefault();void mutate();}}><label>{form.kind==="rename"?"新路径":form.kind==="folder"?"文件夹路径":"文件路径"}<input autoFocus value={name} onChange={e=>setName(e.target.value)} placeholder="相对于项目目录"/></label><button disabled={busy||!name.trim()}>确定</button><button type="button" onClick={()=>setForm(undefined)}>取消</button></form>:null}
      <div className="code-tree">{query?results.map(path=><button key={path} title={path} onClick={()=>void run(()=>open(path))}>{path}</button>):(tree[key(project.id,"")]??[]).map(item=>node(item,0))}</div>
      {query?<small>最多展示 100 项，检索范围为前 10000 个项目文件</small>:null}
    </aside>
    <div className="code-editing"><div className="code-tabs">{Object.entries(buffers).filter(([,f])=>f.projectId===project.id).map(([id,f])=><div key={id} className={id===activeKey?"is-active":""}><button title={f.path} onClick={()=>{setActive(a=>({...a,[project.id]:id}));setCompare(false);setSelected("");}}>{f.content!==f.saved?"● ":""}{f.path.split("/").pop()}</button><button aria-label={`关闭 ${f.path}`} onClick={()=>f.content!==f.saved?setClosing(id):remove(id)}>×</button></div>)}</div>
    {error?<div className="code-notice" role="alert">{error}<button onClick={()=>setError("")}>关闭</button></div>:null}
    {closing?<div className="code-notice">有未保存的修改。<button onClick={()=>setClosing(undefined)}>继续编辑</button><button onClick={()=>remove(closing)}>放弃修改并关闭文件</button></div>:null}
    {file?<><div className="code-editor-toolbar"><span title={file.path}>{file.path}</span><button disabled={busy||file.content===file.saved||!!file.disk} onClick={()=>void save()}>保存</button><button onClick={()=>setCompare(v=>!v)}>{compare?"返回编辑":"查看修改"}</button><button onClick={()=>onAttach(`文件：${file.path}${selected?"（选中内容）":""}\n\n\`\`\`\n${selected||file.content}\n\`\`\``)}>{selected?"选区加入对话":"文件加入对话"}</button></div>
    {file.disk?<div className="code-notice">文件已在外部修改。你的编辑仍保留，请比较后合并。<button onClick={()=>setCompare(true)}>比较</button><button onClick={()=>setReload(true)}>重新加载磁盘文件</button><button onClick={()=>setBuffers(all=>({...all,[activeKey!]:{...file,saved:file.disk!.content,sha256:file.disk!.sha256,disk:undefined}}))}>保留编辑，以磁盘版本为保存基准</button></div>:null}
    {reload&&file.disk?<div className="code-notice">重新加载会放弃当前文件的未保存内容。<button onClick={()=>{const disk=file.disk!;setBuffers(all=>({...all,[activeKey!]:{...file,...disk,saved:disk.content,disk:undefined}}));setReload(false);}}>确认重新加载</button><button onClick={()=>setReload(false)}>取消</button></div>:null}
    <div className={`code-editor-area${compare?" is-comparing":""}`}>{compare?<div className="code-compare-half"><small>{file.disk?"磁盘新版本":"上次保存"}</small><CodeEditor path={file.path} content={file.disk?.content??file.saved} dark={dark} readOnly/></div>:null}<div className="code-compare-half">{compare?<small>当前编辑</small>:null}<CodeEditor key={activeKey} path={file.path} content={file.content} dark={dark} onChange={content=>setBuffers(all=>({...all,[activeKey!]:{...all[activeKey!],content}}))} onSave={()=>void save()} onSelection={setSelected}/></div></div>
    <footer className="code-status">{file.content!==file.saved?"未保存":"已保存"} · UTF-8 · Ctrl/⌘+S 保存 · Ctrl/⌘+F 查找 / 替换</footer></>:<div className="code-empty"><h2>开始编写</h2><p>从左侧选择文件，或把开发任务交给右侧 AI。</p></div>}
    </div></>}
    {closeWindow?createPortal(<div className="code-close-dialog" role="alertdialog" aria-label="未保存的修改"><p>编辑器中还有未保存的修改。请返回 Simple Code 保存。</p><button onClick={()=>setCloseWindow(false)}>继续编辑</button><button onClick={()=>{dirtyRef.current=false;void getCurrentWindow().destroy();}}>放弃修改并退出</button></div>,document.body):null}
  </section>;
}
