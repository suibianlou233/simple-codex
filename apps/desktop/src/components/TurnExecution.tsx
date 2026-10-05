import {useState, type ReactNode} from "react";
import type {TimelineEntry, ToolActionSummary, TurnSummary} from "../bridge/types";
import {elapsedBetween} from "./ElapsedTime";
import "./turnExecution.css";
export function durationLabel(milliseconds: number) {
 const seconds = Math.floor(Math.max(0,milliseconds)/1000);
 const hours = Math.floor(seconds/3600), minutes = Math.floor(seconds%3600/60);
 return [hours ? hours+"小时" : "",minutes ? minutes+"分钟" : "",seconds%60+"秒"].join(" ").trim().replace(/ +/g," ");
}
const statuses: Record<string,string> = {applied:"已完成",completed:"已完成",failed:"未成功",rejected:"已拒绝",cancelled:"已停止",undone:"已撤销",pending:"结果待确认",running:"结果待确认"};
const stepLabels: Record<string,string> = {run_command:"执行命令",command:"执行命令",write_file:"修改文件",file_change:"修改文件",read_file:"读取文件",file_read:"读取文件",error:"操作未成功"};
export function TurnExecution({turn,actions,evidence,children}: {turn:TurnSummary;actions:ToolActionSummary[];evidence:TimelineEntry[];children:ReactNode}) {
 const elapsed=elapsedBetween(turn.startedAt,turn.finishedAt);
 return <details className="turn-execution" data-execution-turn-id={turn.id}>
  <summary><span>{elapsed===null?"用时未记录":"用时 "+durationLabel(elapsed)}</span><span className="turn-execution-chevron" aria-hidden="true">›</span></summary>
  <div className="turn-execution-content">
   {children}
   <ExecutionSteps actions={actions} evidence={evidence} />
  </div>
 </details>;
}

export function ExecutionSteps({actions,evidence=[],live=false}: {actions:ToolActionSummary[];evidence?:TimelineEntry[];live?:boolean}) {
 const rows=actions.length?actions.map(action=>({id:action.id,kind:action.kind,status:action.status,title:action.title,detail:action.detail,result:action.result,diff:action.diff,cwd:action.workingDirectory})):
 evidence.map(entry=>({id:entry.id,kind:entry.kind,status:entry.status??"recorded",title:entry.title,detail:entry.detail,result:null,diff:null,cwd:null}));
 if(!rows.length)return live ? null : <p className="turn-execution-empty">本轮没有工具执行步骤。</p>;
 return <div className="execution-step-list" aria-label="执行步骤">{rows.map((row,index)=><ExecutionStep key={row.id} row={row} index={index} live={live}/>)}</div>;
}
function ExecutionStep({row,index,live}: {row:{id:string;kind:string;status:string;title:string;detail?:string|null;result:string|null;diff:string|null;cwd?:string|null};index:number;live:boolean}) {
 const [open,setOpen]=useState(false);
 const state=live&&row.status==="running"?"正在执行":statuses[row.status]??"已记录";
 return <details className="execution-step" onToggle={event=>setOpen(event.currentTarget.open)}>
  <summary><span className="execution-step-number">{index+1}</span><span>{stepLabels[row.kind]??"执行操作"}</span><small>{state}</small><span aria-hidden="true">›</span></summary>
  {open&&<div className="execution-step-body">
   <p>{row.title}</p>
   {row.cwd&&<p>工作目录：{row.cwd}</p>}
   {row.detail&&<><div>{row.kind==="run_command"||row.kind==="command"?"命令":"操作内容"}</div><pre>{row.detail}</pre></>}
   {row.diff&&<><div>文件变更</div><pre>{row.diff}</pre></>}
   <div>执行结果</div><pre>{row.result??(live&&row.status==="running"?"正在执行，等待输出…":"没有记录输出。")}</pre>
  </div>}
 </details>;
}
