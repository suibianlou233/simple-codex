import {useState} from "react";
import {useCodeAssist} from "./useCodeAssist";
import type {Buffer} from "./editorModel";
import type {PastedTextBlock} from "../app/textDraft";
export function CodeAssistant({projectId,profileId,mode,file,onExecute}:{projectId:string;profileId?:string|null;mode:"discuss"|"plan";file?:Buffer;onExecute:(block:PastedTextBlock)=>void}){
 const storageKey=`simple-code-assist:${projectId}:${mode}`;
 const [prompt,setPrompt]=useState(""),[answer,setAnswer]=useState(()=>{try{return localStorage.getItem(storageKey)??"";}catch{return "";}}),[saveError,setSaveError]=useState("");
 const assist=useCodeAssist(projectId,profileId);
 async function submit(){const result=await assist.ask(mode,prompt,`${file?`项目文件：${file.path}（${file.content!==file.saved?"未保存草稿":"当前内容"}）\n${file.content}`:"未提供文件"}\n上一次答复：\n${answer}`);if(result!==undefined){setAnswer(result);try{localStorage.setItem(storageKey,result);}catch{setSaveError("结果未能持久保存，请复制或加入对话保存");}}}
 return <section className="code-assistant"><header><strong>{mode==="plan"?"制定计划":"讨论代码"}</strong><small>无工具 · 不改文件 · 当前模型</small></header><p>上下文：{file?.path??"未打开文件"}和上一次答复；不会自动读取整个项目。</p><form onSubmit={e=>{e.preventDefault();void submit();}}><textarea aria-label="讨论或计划要求" placeholder={mode==="plan"?"描述开发目标…":"你想了解什么？"} value={prompt} onChange={e=>setPrompt(e.target.value)} disabled={assist.busy}/><button disabled={assist.busy||!prompt.trim()}>{mode==="plan"?"生成计划":"发送讨论"}</button>{assist.busy?<button type="button" onClick={assist.cancel}>取消</button>:null}</form>{assist.error||saveError?<p role="alert">{assist.error||saveError}</p>:null}{answer?<><pre>{answer}</pre><button onClick={()=>onExecute({id:crypto.randomUUID(),label:mode==="plan"?"开发计划":"讨论结论",source:"待执行参考",text:answer})}>加入执行对话</button><small>加入后可继续编辑要求，点击发送才会执行。</small></>:null}</section>;
}
