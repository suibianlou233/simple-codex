import {describe,it,expect} from "vitest";
import {proposalHunks,resolveHunks} from "./editorAiModel";
import {validateSession} from "./editorRecovery";
import {parseFindings} from "./reviewModel";
import {mayDispatch,type QueueJournal} from "../components/ComposerQueue";
import type {TurnSummary} from "../bridge/types";
describe("editor workflow integrity",()=>{
 it("accepts independent hunks without changing CRLF, Unicode or final newline",()=>{const before="old\r\nkeep 😀\r\ndelete\r\nlast",after="new\r\nkeep 😀\r\nlast";const diff=proposalHunks(before,after);expect(diff.hunks).toHaveLength(2);expect(resolveHunks(diff,new Set())).toBe(before);expect(resolveHunks(diff,new Set([0,1]))).toBe(after);expect(resolveHunks(diff,new Set([1]))).toBe("old\r\nkeep 😀\r\nlast");});
 it("rejects corrupt restored drafts instead of overwriting them",()=>{expect(()=>validateSession({version:1,buffers:{wrong:{path:"a",projectId:"p",content:"draft",saved:"disk",sha256:"sha"}},active:{},carets:{}})).toThrow();});
 it("review cannot link outside the project",()=>{for(const path of ["../private","C:/private","/private","a/../private"])expect(()=>parseFindings(JSON.stringify([{path,line:1,severity:"high",title:"bug",detail:"cause"}]))).toThrow();expect(parseFindings("[]")).toEqual([]);});
 it("queue sends only after success and never repeats uncertain or paused sends",()=>{const q:QueueJournal={items:[{id:"one",text:"next"}],waitFor:"turn",notes:[]};const turn={id:"turn",status:"completed"} as TurnSummary;expect(mayDispatch(q,[turn])).toBe(true);for(const status of ["running","failed","cancelled"] as const)expect(mayDispatch(q,[{...turn,status}])).toBe(false);for(const change of [{inflight:"one"},{awaiting:true},{paused:true},{waitFor:"different"}])expect(mayDispatch({...q,...change},[turn])).toBe(false);});
});
