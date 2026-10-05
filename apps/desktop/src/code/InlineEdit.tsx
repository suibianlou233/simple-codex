import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Buffer } from "./editorModel";
import { proposalHunks, resolveHunks, type EditorSelection, type EditProposal } from "./editorAiModel";
import { DiffView } from "./DiffView";

export function InlineEdit({file,selection,profileId,onApply,onClose}:{file:Buffer;selection:EditorSelection;profileId?:string|null;onApply:(p:EditProposal)=>Promise<void>;onClose:()=>void}) {
  const [base]=useState(()=>({...file,selection}));
  const [instruction,setInstruction]=useState("");
  const [busy,setBusy]=useState(false), [applying,setApplying]=useState(false);
  const [error,setError]=useState("");
  const [proposal,setProposal]=useState<EditProposal>();
  const [accepted,setAccepted]=useState<Set<number>>(new Set());
  const diff=useMemo(()=>proposal?proposalHunks(base.content.slice(proposal.start,proposal.end),proposal.replacement):undefined,[proposal,base]);
  useEffect(()=>{setAccepted(new Set(diff?.hunks.map((_,i)=>i)));},[diff]);
  const pending=useRef<string | undefined>(undefined); const alive=useRef(true);
  const cancel=()=>{const requestId=pending.current;if(requestId){pending.current=undefined;void invoke("editor_cancel_suggestion",{projectId:base.projectId,requestId}).catch(()=>{});}setBusy(false);};
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;const requestId=pending.current;if(requestId)void invoke("editor_cancel_suggestion",{projectId:base.projectId,requestId}).catch(()=>{});};},[]);
  const changed=file.content!==base.content || file.sha256!==base.sha256 || !!file.disk;
  async function generate(){
    if(pending.current||changed)return;
    const requestId=crypto.randomUUID();pending.current=requestId;setBusy(true);setError("");setProposal(undefined);
    try {
      const replacement=await invoke<string>("editor_suggest",{input:{projectId:base.projectId,path:base.path,content:base.content,expected:base.sha256,start:base.selection.start,end:base.selection.end,instruction,requestId,profileId:profileId??null}});
      if(alive.current && pending.current===requestId) setProposal({projectId:base.projectId,path:base.path,base:base.content,sha256:base.sha256,start:base.selection.start,end:base.selection.end,replacement});
    }catch(e){if(alive.current && pending.current===requestId)setError(String(e));}
    finally{if(alive.current && pending.current===requestId){pending.current=undefined;setBusy(false);}}
  }
  return <section className="inline-edit" aria-label="AI 选区修改">
    <header><strong>修改选区</strong><small>{base.path} · L{base.selection.startLine}–L{base.selection.endLine}</small><button aria-label="关闭选区修改" onClick={()=>{cancel();onClose();}}>×</button></header>
    <form onSubmit={e=>{e.preventDefault();void generate();}}><input autoFocus value={instruction} onChange={e=>setInstruction(e.target.value)} placeholder="描述修改，例如：增加空值检查" aria-label="选区修改要求" disabled={busy||changed}/><button disabled={busy||changed||!instruction.trim()||applying}>{proposal?"重新生成":"生成建议"}</button>{busy?<button type="button" onClick={cancel}>取消生成</button>:null}</form>
    <small>建议只替换选区；接受后仍需保存文件。</small>
    {busy?<p role="status">正在生成修改建议…</p>:null}
    {changed?<p role="alert">文件已变化，请关闭后重新选择代码。当前建议不会覆盖你的编辑。</p>:null}
    {error?<p role="alert">{error}</p>:null}
    {proposal?<>{diff?.hunks.map((h,i)=><article className="edit-hunk" key={i}><label><input type="checkbox" aria-label={`接受修改块 ${i+1}`} checked={accepted.has(i)} onChange={e=>setAccepted(previous=>{const next=new Set(previous);if(e.target.checked)next.add(i);else next.delete(i);return next;})}/>修改块 {i+1} · 选区第 {h.line} 行</label><DiffView before={h.before} after={h.after}/></article>)}<footer><button disabled={changed||applying||!accepted.size} onClick={()=>{setApplying(true);setError("");void onApply({...proposal,replacement:diff?resolveHunks(diff,accepted):proposal.replacement}).then(onClose).catch(e=>setError(String(e))).finally(()=>{if(alive.current)setApplying(false);});}}>接受修改</button><button disabled={applying} onClick={onClose}>放弃建议</button></footer></>:null}
  </section>;
}
