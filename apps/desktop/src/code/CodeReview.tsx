import {useCodeAssist} from "./useCodeAssist";
import {parseFindings,type Finding} from "./reviewModel";
import type {Location} from "./ProjectSearch";
import { useEffect, useState, useRef } from "react";
import type { DesktopBridge, ToolActionSummary, TurnSummary, GitWorkspaceDiff } from "../bridge/types";
import { PatchView } from "./DiffView";

export function CodeReview({projectId,profileId,onOpen,bridge,taskId,actions,turns,onClose}:{projectId:string;profileId?:string|null;onOpen:(location:Location)=>void;bridge:DesktopBridge;taskId?:string;actions:ToolActionSummary[];turns:TurnSummary[];onClose:()=>void}) {
  const assist=useCodeAssist(projectId,profileId);
  const [findings,setFindings]=useState<Finding[]>(),[reviewError,setReviewError]=useState("");
  const [scope,setScope]=useState<"turn"|"git">("turn");
  const [git,setGit]=useState<GitWorkspaceDiff>(), [error,setError]=useState("");
  const [revision,setRevision]=useState(0),[busy,setBusy]=useState(false);
  useEffect(()=>{if(scope!=="git"||!taskId)return;let alive=true;setBusy(true);setError("");setGit(undefined);void bridge.loadWorkspaceDiff(taskId).then(value=>{if(alive)setGit(value);}).catch(e=>{if(alive)setError(String(e));}).finally(()=>{if(alive)setBusy(false);});return()=>{alive=false;};},[bridge,taskId,scope,revision]);
  const latest=turns.at(-1)?.id;
  const changes=actions.filter(a=>a.turnId===latest && a.kind==="write_file");
  const reviewText=scope==="git"?git?.unifiedDiff??"":changes.map(a=>a.diff??"").join("\n");
  const currentReview=useRef(reviewText);currentReview.current=reviewText;
  useEffect(()=>{setFindings(undefined);setReviewError("");},[reviewText,scope]);
  return <section className="code-review" aria-label="任务修改审查"><header><strong>修改审查</strong><button onClick={onClose}>返回编辑</button></header><nav><button aria-pressed={scope==="turn"} disabled={assist.busy} onClick={()=>setScope("turn")}>最近一轮 AI 修改</button><button aria-pressed={scope==="git"} disabled={assist.busy} onClick={()=>setScope("git")}>Git 工作区</button></nav>
    <button disabled={assist.busy||!reviewText} onClick={()=>{setFindings(undefined);setReviewError("");void assist.ask("review","审查这些修改，给出可定位的缺陷和原因。",reviewText).then(text=>{if(text!==undefined)try{if(currentReview.current!==reviewText)throw new Error("差异已经变化，请重新审查");setFindings(parseFindings(text));}catch(e){setReviewError(String(e));}});}}>AI 审查当前差异</button>{assist.busy?<button onClick={assist.cancel}>取消审查</button>:null}<small>仅分析当前展示的补丁，不修改代码；位置对应补丁生成时的文件。</small>{assist.error||reviewError?<p role="alert">{assist.error||reviewError}</p>:null}{findings?<div className="review-findings">{findings.length?findings.map((f,i)=><article key={i}><button onClick={()=>onOpen(f)}>{f.severity} · {f.path}:{f.line} · {f.title}</button><p>{f.detail}</p></article>):<p>本次审查未报告具体缺陷；这不代表已经通过测试。</p>}</div>:null}
    {!taskId?<p>选择一个项目对话后查看修改。</p>:scope==="turn"?<><p>展示内核记录的文件修改。命令产生的修改可能没有补丁记录，可在 Git 工作区核对。</p>{!changes.length?<p>这一轮没有可展示的文件修改记录。</p>:null}{changes.map(action=><article key={action.id}><h3>{action.title}</h3><small>{({applied:"已写入磁盘",pending:"待确认，尚未应用",running:"执行中",failed:"执行未成功",rejected:"已拒绝",undone:"已撤销",cancelled:"已取消",unknown:"结果待确认"} as Record<string,string>)[action.status]??action.status}</small>{action.diff?<PatchView patch={action.diff}/>:<p>内核未提供补丁内容，请查看 Git 工作区。</p>}</article>)}</>:<><p>与 HEAD 比较的全部工作区修改，包含你手动修改的内容，不代表全部由 AI 产生。未跟踪文件可能仅列出名称。</p><button disabled={busy} onClick={()=>setRevision(v=>v+1)}>刷新差异</button>{busy?<p role="status">读取 Git 差异…</p>:null}{error?<p role="alert">{error}</p>:null}{git?<><p>{git.summary}</p><ul>{git.files.map(file=><li key={file.path}>{file.path} {file.statisticsAvailable!==false?`+${file.additions} −${file.deletions}`:""}</li>)}</ul>{git.unifiedDiff?<PatchView patch={git.unifiedDiff}/>:null}</>:null}</>}
  </section>;
}
