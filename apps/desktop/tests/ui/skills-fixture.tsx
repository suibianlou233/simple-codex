import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { SkillManager } from "../../src/settings/SkillManager";
import "../../src/styles.css";
const library: {id:string;name:string;description:string;enabled:boolean;issues:string[];files:number}[]=[];
mockIPC((cmd,args) => {
  if(cmd==="skills_library_list") return structuredClone(library);
  if(cmd==="skills_codex_list") return [{id:"candidate",name:"write-serialized-novel",description:"规划与续写长篇中文小说"}];
  if(cmd==="skills_import") {if(!library.length) library.push({id:"write-serialized-novel",name:"write-serialized-novel",description:"规划与续写长篇中文小说",enabled:true,issues:[],files:6});return library[0];}
  if(cmd==="skills_set_enabled") {library[0].enabled=Boolean((args as Record<string, unknown> | undefined)?.enabled);return null;}
  if(cmd==="skills_document") return "# 小说技能\n<script>window.SKILL_EXECUTED=true</script>";
  throw new Error("Unexpected IPC");
});
createRoot(document.getElementById("root")!).render(<SkillManager onClose={() => {document.getElementById("root")!.textContent="已关闭";}} />);
