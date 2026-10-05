import {useEffect,useRef,useState} from "react";
import {invoke} from "@tauri-apps/api/core";
export function useCodeAssist(projectId:string,profileId?:string|null){
 const [busy,setBusy]=useState(false),[error,setError]=useState("");const pending=useRef<string|undefined>(undefined),alive=useRef(true);
 const cancel=()=>{const requestId=pending.current;pending.current=undefined;if(requestId)void invoke("editor_cancel_suggestion",{projectId,requestId}).catch(()=>{});setBusy(false);};
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;const requestId=pending.current;pending.current=undefined;if(requestId)void invoke("editor_cancel_suggestion",{projectId,requestId}).catch(()=>{});};},[projectId]);
 const ask=async(mode:"discuss"|"plan"|"review",prompt:string,context:string)=>{if(pending.current)return;const requestId=crypto.randomUUID();pending.current=requestId;setBusy(true);setError("");try{const result=await invoke<string>("editor_assist",{input:{projectId,profileId:profileId??null,requestId,mode,prompt,context}});if(alive.current&&pending.current===requestId)return result;}catch(e){if(alive.current&&pending.current===requestId)setError(String(e));}finally{if(alive.current&&pending.current===requestId){pending.current=undefined;setBusy(false);}}};
 return {busy,error,ask,cancel};
}
