import {useRef,useState} from "react";
import {createRoot} from "react-dom/client";
import {Composer} from "../../src/components/Composer";
import {ComposerQueueProvider} from "../../src/components/ComposerQueue";
import type {DesktopBridge,TurnSummary} from "../../src/bridge/types";
import "../../src/styles.css";
const steers:unknown[]=[];Object.assign(window,{__steers:steers});
function Fixture(){
 const [turns,setTurns]=useState<TurnSummary[]>([{id:"first",taskId:"task",status:"running",phase:"sampling",sequence:1,startedAt:"",finishedAt:null}]);
 const [text,setText]=useState(""),[sent,setSent]=useState<string[]>([]),[fail,setFail]=useState(false);const sequence=useRef(1);
 const bridge={sendMessage:async(_:string,text:string)=>{setSent(old=>[...old,text]);if(fail)throw new Error("unknown response");const n=++sequence.current;setTurns(old=>[...old,{id:String(n),taskId:"task",status:"running",phase:"sampling",sequence:n,startedAt:"",finishedAt:null}]);}} as DesktopBridge;
 const active=turns.find(t=>t.status==="running");
 return <><button onClick={()=>setTurns(old=>old.map(t=>({...t,status:"completed",phase:"completed"})))}>完成当前轮</button><button onClick={()=>setTurns(old=>old.map(t=>({...t,status:"failed",phase:"failed"})))}>失败当前轮</button><label><input type="checkbox" checked={fail} onChange={e=>setFail(e.target.checked)}/>模拟发送结果不明</label>
 <ComposerQueueProvider taskId="task" bridge={bridge} turns={turns} onSteer={async(turnId,content)=>{steers.push({turnId,content});}}>
 <Composer projectId="project" activeTurnId={active?.id??null} phase={active?.phase} disabled={false} content={text} onContentChange={setText} attachments={[]} onAttach={async()=>{}} onRemoveAttachment={()=>{}} onQueued={()=>setText("")} onSend={async message=>{await bridge.sendMessage("task",message);setText("");}} onCancel={async()=>true} permissionLevel="approval" permissionScopeLabel="当前任务" workspaceSandboxReady={false} permissionDisabled={true} onPermissionChange={async()=>true}/>
 </ComposerQueueProvider><output aria-label="已发送消息">{JSON.stringify(sent)}</output></>;
}
createRoot(document.getElementById("root")!).render(<Fixture/>);
