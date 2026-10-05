import type { Buffer } from "./editorModel";
export type EditorSession = {version:1; buffers:Record<string,Buffer>;active:Record<string,string>;carets:Record<string,{line:number;column:number}>};
export const emptySession = ():EditorSession=>({version:1,buffers:{},active:{},carets:{}});
function database():Promise<IDBDatabase>{return new Promise((resolve,reject)=>{const req=indexedDB.open("simple-editor-drafts",1);req.onupgradeneeded=()=>req.result.createObjectStore("sessions");req.onsuccess=()=>resolve(req.result);req.onerror=()=>reject(req.error);});}
export function validateSession(value:unknown):EditorSession {
  const s=value as EditorSession;if(!s||s.version!==1||!s.buffers||!s.active||!s.carets)throw new Error("编辑器恢复记录格式无效");
  for(const [key,f] of Object.entries(s.buffers))if(!f||typeof f.path!=="string"||typeof f.projectId!=="string"||typeof f.content!=="string"||typeof f.saved!=="string"||typeof f.sha256!=="string"||key!==f.projectId+"\0"+f.path)throw new Error("编辑器恢复记录损坏");
  for(const caret of Object.values(s.carets))if(!caret||!Number.isSafeInteger(caret.line)||!Number.isSafeInteger(caret.column)||caret.line<1||caret.column<1)throw new Error("恢复记录中的光标位置无效");
  return s;
}
export async function readSession():Promise<EditorSession>{const db=await database();try{return await new Promise((resolve,reject)=>{const req=db.transaction("sessions").objectStore("sessions").get("current");req.onsuccess=()=>{try{resolve(req.result?validateSession(req.result):emptySession());}catch(e){reject(e);}};req.onerror=()=>reject(req.error);});}finally{db.close();}}
// Serialize writes so an older caret update cannot overwrite a newer draft.
let pending=Promise.resolve();
export function writeSession(session:EditorSession):Promise<void>{const next=pending.catch(()=>{}).then(async()=>{const db=await database();try{await new Promise<void>((resolve,reject)=>{const tx=db.transaction("sessions","readwrite");tx.objectStore("sessions").put(session,"current");tx.oncomplete=()=>resolve();tx.onerror=()=>reject(tx.error);tx.onabort=()=>reject(tx.error);});}finally{db.close();}});pending=next;return next;}
