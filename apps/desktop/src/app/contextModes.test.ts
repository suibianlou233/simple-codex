import {it,expect} from "vitest";
import {MemoryDesktopBridge} from "../bridge/memoryBridge";
import {contextKey,snapshotForMode} from "./contextModes";
it("creates separate histories, rejects cross-mode sends and keeps branches in scope",async()=>{
 const bridge=new MemoryDesktopBridge();await bridge.openProject();await bridge.saveModelProfile({name:"test",model:"test",baseUrl:"http://127.0.0.1/unused",dialect:"standard",timeoutMs:30000,isDefault:true});const project=(await bridge.load()).projects[0].id;
 const simple=await bridge.startChat(project,"simple secret","approval","simple"),code=await bridge.startChat(project,"code secret","approval","code");
 await expect(bridge.sendMessage(simple.taskId,"wrong target","code")).rejects.toThrow("工作模式");
 const snapshot=await bridge.load(),codeView=snapshotForMode(snapshot,"code"),simpleView=snapshotForMode(snapshot,"simple");
 expect(codeView.tasks.map(t=>t.id)).toEqual([code.taskId]);expect(simpleView.tasks.map(t=>t.id)).toEqual([simple.taskId]);expect(codeView.timeline.some(e=>e.detail?.includes("simple secret"))).toBe(false);
 expect(contextKey(project,"simple")).not.toBe(contextKey(project,"code"));
 const message=codeView.timeline.find(e=>e.kind==="user")!;await bridge.branchConversation(code.taskId,message.id);expect((await bridge.load()).tasks[0].contextMode).toBe("code");
});
