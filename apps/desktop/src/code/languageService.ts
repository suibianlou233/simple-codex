import {invoke} from "@tauri-apps/api/core";
import type {Buffer} from "./editorModel";
import type {Location} from "./ProjectSearch";
let worker:Worker|undefined,id=0;
const indexes=new Map<string,{at:number;request:Promise<{files:{path:string;content:string}[];limited:boolean}>}>();
function projectFiles(projectId:string){let cached=indexes.get(projectId);if(!cached||Date.now()-cached.at>3000){if(indexes.size>3)indexes.clear();cached={at:Date.now(),request:invoke("editor_language_files",{projectId})};indexes.set(projectId,cached);void cached.request.catch(()=>indexes.delete(projectId));}return cached.request;}
const pending=new Map<number,{resolve:(v:LanguageResult)=>void;reject:(e:unknown)=>void;timer:ReturnType<typeof setTimeout>}>();
export type LanguageResult={hover?:string;locations?:Location[];limited?:boolean};
export async function queryLanguage(projectId:string,path:string,offset:number,kind:"definition"|"references"|"hover",buffers:Record<string,Buffer>):Promise<LanguageResult>{
 if(!/\.[cm]?[jt]sx?$/.test(path))throw new Error("语义导航目前支持 JavaScript / TypeScript；其他语言可用项目搜索");
 if(!worker){worker=new Worker(new URL("./language.worker.ts",import.meta.url),{type:"module"});worker.onmessage=e=>{const p=pending.get(e.data.id);if(p){clearTimeout(p.timer);pending.delete(e.data.id);if(e.data.error)p.reject(e.data.error);else p.resolve(e.data);}};worker.onerror=e=>{for(const p of pending.values()){clearTimeout(p.timer);p.reject(e.message);}pending.clear();worker?.terminate();worker=undefined;};}
 const data=await projectFiles(projectId);
 const files=new Map(data.files.map(f=>[f.path,f]));for(const f of Object.values(buffers))if(f.projectId===projectId&&/\.[cm]?[jt]sx?$/.test(f.path))files.set(f.path,{path:f.path,content:f.content});
 const requestId=++id;const result=await new Promise<LanguageResult>((resolve,reject)=>{const timer=setTimeout(()=>{pending.delete(requestId);reject(new Error("语义分析超时，请缩小项目"));},15000);pending.set(requestId,{resolve,reject,timer});worker!.postMessage({id:requestId,files:Array.from(files.values()),path,offset,kind});});return {...result,limited:data.limited};
}
