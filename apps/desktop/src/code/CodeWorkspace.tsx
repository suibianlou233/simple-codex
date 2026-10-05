import SplitPane from "@dtinsight/molecule/esm/components/split/SplitPane";
import {Tabs} from "@dtinsight/molecule/esm/components/tabs";
import "@dtinsight/molecule/esm/components/split/style.css";
import "@dtinsight/molecule/esm/components/tabs/style.css";
import "@dtinsight/molecule/esm/components/scrollBar/style.css";
import "./molecule.css";
import {EditorMenu,type EditorMenuItem} from "./EditorMenu";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {readSession,writeSession,emptySession} from "./editorRecovery";
import {ProjectSearch,type Location} from "./ProjectSearch";
import { CodeEditor } from "./CodeEditor";
import { InlineEdit } from "./InlineEdit";
import { FileReferencePicker } from "./FileReferencePicker";
import {CodeAssistant} from "./CodeAssistant";
import { CodeReview } from "./CodeReview";
import { DiffView } from "./DiffView";
import { applyProposal, fileReference, type EditorSelection, type EditProposal } from "./editorAiModel";
import type { PastedTextBlock } from "../app/textDraft";
import type { DesktopBridge, ToolActionSummary, TurnSummary } from "../bridge/types";
import { reconcile, type Buffer, type EditorFile } from "./editorModel";
import type { ProjectSummary } from "../bridge/types";
type Entry = {path:string; name:string; directory:boolean};
const key = (project:string,path:string)=>`${project}\0${path}`;
export function CodeWorkspace({project,visible,dark,onOpenProject,onAttach,request,onModalVisibility,referenceRequest,profileId,bridge,taskId,actions,turns}:{referenceRequest?:number;profileId?:string|null;bridge:DesktopBridge;taskId?:string;actions:ToolActionSummary[];turns:TurnSummary[];project?:ProjectSummary;visible:boolean;dark:boolean;onOpenProject:()=>void;onAttach:(block:PastedTextBlock)=>void;request?:{path:string;nonce:number;line?:number;column?:number};onModalVisibility:(open:boolean)=>void}) {
  const [paneSizes,setPaneSizes]=useState<(string|number)[]>([220,"auto"]);
  const [menu,setMenu]=useState<{x:number;y:number}>();
  const [navigation,setNavigation]=useState<{kind:"definition"|"references";nonce:number}>();
  const [ready,setReady]=useState(false),[recoveryError,setRecoveryError]=useState("");
  const [carets,setCarets]=useState<Record<string,{line:number;column:number}>>({});
  const [reveal,setReveal]=useState<{line:number;column?:number;nonce:number}>();
  const [searchOpen,setSearchOpen]=useState(false);
  const [buffers,setBuffers]=useState<Record<string,Buffer>>({});
  const [active,setActive]=useState<Record<string,string>>({});
  const [tree,setTree]=useState<Record<string,Entry[]>>({});
  const [expanded,setExpanded]=useState<Set<string>>(new Set());
  const [query,setQuery]=useState(""); const [results,setResults]=useState<string[]>([]);
  const [error,setError]=useState(""); const [busy,setBusy]=useState(false);
  const [form,setForm]=useState<{kind:"file"|"folder"|"rename";path:string}>(); const [name,setName]=useState("");
  const [selected,setSelected]=useState<EditorSelection>();
  const [inline,setInline]=useState<{id:string;selection:EditorSelection}>();
  const [referencePicker,setReferencePicker]=useState(false);
  const [review,setReview]=useState(false);
  const [assistantMode,setAssistantMode]=useState<"execute"|"discuss"|"plan">("execute");
  useEffect(()=>{if(referenceRequest)setReferencePicker(true);},[referenceRequest]);
  useEffect(()=>{setInline(undefined);setReferencePicker(false);setReview(false);},[project?.id]); const [compare,setCompare]=useState(false);
  const [closing,setClosing]=useState<string>(); const [closeWindow,setCloseWindow]=useState(false);
  useEffect(()=>{onModalVisibility(closeWindow||referencePicker);return()=>onModalVisibility(false);},[closeWindow,referencePicker,onModalVisibility]);
  const [reload,setReload]=useState(false);
  useEffect(()=>{let alive=true;void readSession().then(s=>{if(alive){setBuffers(s.buffers);setActive(s.active);setCarets(s.carets);setReady(true);}}).catch(e=>{if(alive)setRecoveryError(String(e));});return()=>{alive=false;};},[]);
  useEffect(()=>{if(ready)void writeSession({version:1,buffers,active,carets}).catch(e=>setRecoveryError(`自动恢复保存失败：${String(e)}`));},[ready,buffers,active,carets]);
  const state=useRef(buffers);state.current=buffers;
  const projectId=project?.id;
  const activeKey=projectId?active[projectId]:undefined; const file=activeKey?buffers[activeKey]:undefined;
  const dirty=Object.values(buffers).some(f=>f.content!==f.saved);
  const dirtyRef=useRef(dirty);dirtyRef.current=dirty;
  useEffect(()=>{
    const before=(event:BeforeUnloadEvent)=>{if(dirtyRef.current){event.preventDefault();event.returnValue="";}};
    window.addEventListener("beforeunload",before);
    let disposed=false; let unlisten:(()=>void)|undefined;
    if("__TAURI_INTERNALS__" in window) void getCurrentWindow().onCloseRequested(event=>{if(dirtyRef.current){event.preventDefault();setCloseWindow(true);}}).then(stop=>{if(disposed)stop();else unlisten=stop;}).catch(e=>setRecoveryError(`关闭事件监听失败：${String(e)}`));
    return ()=>{disposed=true;unlisten?.();window.removeEventListener("beforeunload",before);};
  },[]);
  async function list(path="") {if(!projectId)return; const entries=await invoke<Entry[]>("editor_list",{projectId,path});setTree(t=>({...t,[key(projectId,path)]:entries}));}
  async function run(action:()=>Promise<void>) {setBusy(true);setError("");try{await action();}catch(e){setError(String(e));}finally{setBusy(false);}}
  async function open(path:string,location?:{line?:number;column?:number}) {
    setNavigation(undefined);
    if(!projectId)return; const id=key(projectId,path);
    if(!state.current[id]) {const data=await invoke<EditorFile>("editor_read",{projectId,path});setBuffers(all=>all[id]?all:{...all,[id]:{...data,projectId,saved:data.content}});}
    setActive(all=>({...all,[projectId]:id}));setReveal(location?.line?{line:location.line,column:location.column,nonce:Date.now()}:undefined);setInline(undefined);setReview(false);setSelected(undefined);setCompare(false);setReload(false);
  }
  useEffect(()=>{setQuery("");setForm(undefined);setError("");if(projectId&&visible)void run(()=>list());},[projectId,visible]);
  useEffect(()=>{if(request&&projectId&&ready)void run(()=>open(request.path,request));},[request,projectId,ready]);
  useEffect(()=>{if(!projectId||!query.trim()){setResults([]);return;}let cancelled=false;
    const timer=setTimeout(()=>{void invoke<string[]>("editor_search",{projectId,query}).then(paths=>{if(!cancelled)setResults(paths);}).catch(e=>{if(!cancelled)setError(String(e));});},180);
    return()=>{cancelled=true;clearTimeout(timer);};},[query,projectId]);
  useEffect(()=>{if(!projectId||!visible||!ready)return;let disposed=false,reading=false;
    const check=async()=>{if(reading)return;reading=true;try{
      const dirs=["",...Array.from(expanded).filter(id=>id.startsWith(projectId+"\0")).map(id=>id.slice(projectId.length+1))];
      await Promise.all(dirs.map(async path=>{try{const entries=await invoke<Entry[]>("editor_list",{projectId,path});if(!disposed)setTree(t=>({...t,[key(projectId,path)]:entries}));}catch{if(!disposed)setTree(t=>({...t,[key(projectId,path)]:[]}));}}));
      await Promise.all(Object.entries(state.current).filter(([,f])=>f.projectId===projectId).map(async([id,f])=>{try{const disk=await invoke<EditorFile>("editor_read",{projectId,path:f.path});if(!disposed)setBuffers(all=>{const current=all[id];if(!current||current.sha256!==f.sha256)return all;if(!current.unavailable&&(current.sha256===disk.sha256||current.disk?.sha256===disk.sha256))return all;return {...all,[id]:{...reconcile(current,disk),unavailable:undefined}};});}catch(e){if(!disposed)setBuffers(all=>all[id]&&all[id].unavailable!==String(e)?{...all,[id]:{...all[id],unavailable:String(e)}}:all);}}));
    }finally{reading=false;}};
    const timer=setInterval(()=>void check(),1500);window.addEventListener("focus",check);void check();return()=>{disposed=true;clearInterval(timer);window.removeEventListener("focus",check);};
  },[projectId,visible,ready,expanded]);
  const openLocation=(location:Location)=>void run(()=>open(location.path,location));
  async function save(){if(!file||!activeKey||busy||(file.disk||file.unavailable))return;const id=activeKey;const submitted=file.content;
    await run(async()=>{const data=await invoke<EditorFile>("editor_save",{projectId:file.projectId,path:file.path,content:submitted,expected:file.sha256});setBuffers(all=>all[id]?({...all,[id]:{...all[id],sha256:data.sha256,saved:data.content,disk:undefined}}):all);});
  }
  async function applyInline(proposal:EditProposal) {
    const id=key(proposal.projectId,proposal.path);
    const current=state.current[id];
    if(!current)throw new Error("文件已关闭，建议未应用");
    applyProposal(current,proposal);
    const disk=await invoke<EditorFile>("editor_read",{projectId:proposal.projectId,path:proposal.path});
    if(disk.sha256!==proposal.sha256) {
      setBuffers(all=>all[id]?({...all,[id]:reconcile(all[id],disk)}):all);
      throw new Error("文件已在磁盘上变化，建议未应用，请先处理冲突");
    }
    const latest=state.current[id];
    if(!latest)throw new Error("文件已关闭，建议未应用");
    const content=applyProposal(latest,proposal);
    setBuffers(all=>all[id]===latest?({...all,[id]:{...latest,content}}):all);
    setSelected(undefined);
  }
  async function attachPath(path:string) {
    if(!projectId)return;
    const file=state.current[key(projectId,path)];
    if(file) {onAttach(fileReference(file));return;}
    const data=await invoke<EditorFile>("editor_read",{projectId,path});
    onAttach(fileReference({...data,projectId,saved:data.content}));
  }
  function beginInline(){if(file&&activeKey&&selected&&selected.end>selected.start&&!file.disk)setInline({id:activeKey,selection:{...selected}});}
  function remove(id:string){const removed=buffers[id];setBuffers(all=>{const next={...all};delete next[id];return next;});if(removed&&active[removed.projectId]===id)setActive(all=>({...all,[removed.projectId]:Object.keys(buffers).find(other=>other!==id&&buffers[other].projectId===removed.projectId)??""}));setClosing(undefined);}
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
  const menuItems:EditorMenuItem[]=[
    {label:"保存",shortcut:"Ctrl / ⌘ S",disabled:!file||busy||file.content===file.saved||!!file.disk||!!file.unavailable,run:()=>void save()},
    {label:compare?"返回编辑":"查看修改",disabled:!file,run:()=>{setAssistantMode("execute");setReview(false);setCompare(v=>!v);}},
    {label:"关闭当前文件",disabled:!file,run:()=>{if(file&&activeKey){if(file.content!==file.saved)setClosing(activeKey);else remove(activeKey);}}},
    {label:"AI 修改选区",shortcut:"Ctrl / ⌘ K",separator:true,disabled:!file||!selected||selected.start===selected.end||!!file.disk,run:beginInline},
    {label:selected&&selected.end>selected.start?"选区加入对话":"文件加入对话",disabled:!file,run:()=>{if(file)onAttach(fileReference(file,selected));}},
    {label:"引用项目文件",shortcut:"@",run:()=>setReferencePicker(true)},
    {label:"转到定义",shortcut:"F12",separator:true,disabled:!file,run:()=>{setAssistantMode("execute");setReview(false);setCompare(false);setNavigation({kind:"definition",nonce:Date.now()});}},
    {label:"查找引用",shortcut:"Shift F12",disabled:!file,run:()=>{setAssistantMode("execute");setReview(false);setCompare(false);setNavigation({kind:"references",nonce:Date.now()});}},
    {label:"任务修改",separator:true,run:()=>setReview(true)},
    {label:"讨论代码",run:()=>{setAssistantMode("discuss");setReview(false);}},
    {label:"制定计划",run:()=>{setAssistantMode("plan");setReview(false);}},
    {label:"返回代码",disabled:assistantMode==="execute"&&!review&&!compare,run:()=>{setAssistantMode("execute");setReview(false);setCompare(false);}},
    ...Object.entries(buffers).filter(([,f])=>f.projectId===projectId).map(([id,f],i)=>({label:`${id===activeKey?"✓ ":""}${f.path}${f.content!==f.saved?" ●":""}`,separator:i===0,run:()=>{void open(f.path);setAssistantMode("execute");}})),
  ];
  if(!ready)return <section className="code-workspace" hidden={!visible}><div className="code-empty"><p>{recoveryError||"恢复编辑器草稿…"}</p>{recoveryError?<button onClick={()=>{void writeSession(emptySession()).then(()=>{setReady(true);setRecoveryError("");}).catch(e=>setRecoveryError(String(e)));}}>放弃损坏的恢复记录并继续</button>:null}</div></section>;
  return <section className="code-workspace" hidden={!visible} aria-label="Simple Code 编辑区">
    {!project?<div className="code-empty"><h2>Simple Code</h2><p>打开一个项目，开始阅读和修改代码。</p><button onClick={onOpenProject}>打开文件夹</button></div>:<SplitPane className="simple-molecule" sizes={paneSizes} onChange={sizes=>setPaneSizes([Math.max(160, Math.min(sizes[0], sizes.reduce((a,b)=>a+b,0)-220)), "auto"])} split="vertical">
    <aside className="code-files"><header><strong>文件</strong><button aria-pressed={searchOpen} onClick={()=>setSearchOpen(!searchOpen)} title="全项目正文搜索">搜索</button><button title="刷新文件树" onClick={()=>void run(async()=>{setTree({});setExpanded(new Set());await list();})}>↻</button><button title="新建文件" onClick={()=>{setForm({kind:"file",path:""});setName("");}}>＋</button><button title="新建文件夹" onClick={()=>{setForm({kind:"folder",path:""});setName("");}}>▣＋</button></header>
      {searchOpen?<ProjectSearch projectId={project.id} onOpen={openLocation}/>:null}
      <input aria-label="搜索项目文件" placeholder="查找文件…" value={query} onChange={e=>setQuery(e.target.value)}/>
      {form?<form className="code-path-form" onSubmit={e=>{e.preventDefault();void mutate();}}><label>{form.kind==="rename"?"新路径":form.kind==="folder"?"文件夹路径":"文件路径"}<input autoFocus value={name} onChange={e=>setName(e.target.value)} placeholder="相对于项目目录"/></label><button disabled={busy||!name.trim()}>确定</button><button type="button" onClick={()=>setForm(undefined)}>取消</button></form>:null}
      <div className="code-tree">{query?results.map(path=><button key={path} title={path} onClick={()=>void run(()=>open(path))}>{path}</button>):(tree[key(project.id,"")]??[]).map(item=>node(item,0))}</div>
      {query?<small>最多展示 100 项，检索范围为前 10000 个项目文件</small>:null}
    </aside>
    <div className="code-editing" onContextMenu={e=>{if((e.target as HTMLElement).closest("input,textarea,.inline-edit,.code-assistant"))return;e.preventDefault();setMenu({x:e.clientX,y:e.clientY});}} onKeyDown={e=>{if((e.ctrlKey||e.metaKey)&&e.shiftKey&&e.key.toLowerCase()==="p"){e.preventDefault();e.stopPropagation();const rect=e.currentTarget.getBoundingClientRect();setMenu({x:rect.left+16,y:rect.top+12});}}}>
    <Tabs role="tablist" type="card" activeTab={activeKey} data={Object.entries(buffers).filter(([,f])=>f.projectId===projectId).map(([id,f])=>({id,name:f.path.split("/").pop(),status:f.content!==f.saved?"edited" as const:undefined,closable:true}))} onSelectTab={id=>{const target=buffers[String(id)];if(target){void open(target.path);setAssistantMode("execute");}}} onCloseTab={id=>{const target=buffers[String(id)];if(target?.content!==target?.saved)setClosing(String(id));else remove(String(id));}}/>
    {file?<div className="molecule-breadcrumb">{project.name}<span>›</span>{file.path.split("/").join("  ›  ")}</div>:null}
    {recoveryError?<div className="code-notice" role="alert">{recoveryError}</div>:null}
    {error?<div className="code-notice" role="alert">{error}<button onClick={()=>setError("")}>关闭</button></div>:null}
    {closing?<div className="code-notice">有未保存的修改。<button onClick={()=>setClosing(undefined)}>继续编辑</button><button onClick={()=>remove(closing)}>放弃修改并关闭文件</button></div>:null}
    {assistantMode!=="execute"&&!review?<CodeAssistant key={project.id+assistantMode} projectId={project.id} profileId={profileId} mode={assistantMode} file={file} onExecute={block=>{onAttach(block);setAssistantMode("execute");}}/>:review?<CodeReview key={taskId??"none"} projectId={project.id} profileId={profileId} onOpen={openLocation} bridge={bridge} taskId={taskId} actions={actions} turns={turns} onClose={()=>setReview(false)}/>:file?<>
    {file.unavailable?<div className="code-notice" role="alert">文件已删除或不可读，编辑内容仍保留：{file.unavailable}</div>:null}
    {file.disk?<div className="code-notice">文件已在外部修改。你的编辑仍保留，请比较后合并。<button onClick={()=>setCompare(true)}>比较</button><button onClick={()=>setReload(true)}>重新加载磁盘文件</button><button onClick={()=>setBuffers(all=>({...all,[activeKey!]:{...file,saved:file.disk!.content,sha256:file.disk!.sha256,disk:undefined}}))}>保留编辑，以磁盘版本为保存基准</button></div>:null}
    {reload&&file.disk?<div className="code-notice">重新加载会放弃当前文件的未保存内容。<button onClick={()=>{const disk=file.disk!;setBuffers(all=>({...all,[activeKey!]:{...file,...disk,saved:disk.content,disk:undefined}}));setReload(false);}}>确认重新加载</button><button onClick={()=>setReload(false)}>取消</button></div>:null}
    {inline&&inline.id===activeKey?<InlineEdit key={inline.id+":"+inline.selection.start+":"+inline.selection.end} file={file} selection={inline.selection} profileId={profileId} onApply={applyInline} onClose={()=>setInline(undefined)}/>:null}
    <div className={`code-editor-area${compare?" is-comparing":""}`}>{compare?<DiffView before={file.disk?.content??file.saved} after={file.content}/>:<div className="code-compare-half"><CodeEditor navigation={navigation} key={activeKey} path={file.path} content={file.content} dark={dark} projectId={project.id} buffers={buffers} onNavigate={openLocation} onChange={content=>setBuffers(all=>({...all,[activeKey!]:{...all[activeKey!],content}}))} onSave={()=>void save()} initialCaret={carets[activeKey!]} reveal={reveal} onCaret={caret=>setCarets(all=>({...all,[activeKey!]:caret}))} onSelection={setSelected} onInlineEdit={beginInline}/></div>}</div>
</>:<div className="code-empty"><h2>开始编写</h2><p>从左侧选择文件，或把开发任务交给右侧 AI。</p></div>}
        <footer className="code-status"><span title={file?.path}>{file?.path??project.name}{file&&file.content!==file.saved?" · 未保存":""}</span><span>{carets[activeKey!]?`Ln ${carets[activeKey!].line}, Col ${carets[activeKey!].column}`:"UTF-8"}</span><button aria-label="编辑器操作" title="编辑器操作 · 右键 / Ctrl+Shift+P" onClick={e=>{const rect=e.currentTarget.getBoundingClientRect();setMenu({x:rect.right-260,y:rect.top});}}>···</button></footer>
    </div></SplitPane>}
    {menu?<EditorMenu x={menu.x} y={menu.y} items={menuItems} onClose={()=>setMenu(undefined)}/>:null}
    {referencePicker&&projectId?createPortal(<FileReferencePicker key={projectId} projectId={projectId} onPick={attachPath} onClose={()=>setReferencePicker(false)}/>,document.body):null}
    {closeWindow?createPortal(<div className="code-close-dialog" role="alertdialog" aria-label="未保存的修改"><p>编辑器中还有未保存的修改。请返回 Simple Code 保存。</p>{recoveryError?<p role="alert">{recoveryError}</p>:null}<button onClick={()=>setCloseWindow(false)}>继续编辑</button><button onClick={()=>{void writeSession({version:1,buffers:state.current,active,carets}).then(()=>{dirtyRef.current=false;return getCurrentWindow().destroy();}).catch(e=>{dirtyRef.current=Object.values(state.current).some(f=>f.content!==f.saved);setRecoveryError(String(e));});}}>保留草稿并退出</button><button onClick={()=>{const kept=Object.fromEntries(Object.entries(state.current).filter(([,f])=>f.content===f.saved));void writeSession({version:1,buffers:kept,active,carets}).then(()=>{dirtyRef.current=false;return getCurrentWindow().destroy();}).catch(e=>{dirtyRef.current=Object.values(state.current).some(f=>f.content!==f.saved);setRecoveryError(String(e));});}}>放弃修改并退出</button></div>,document.body):null}
  </section>;
}
