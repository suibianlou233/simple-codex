import { describe, it, expect } from "vitest";
import { applyProposal, fileReference, lineDiff, type EditProposal } from "./editorAiModel";
import type { Buffer } from "./editorModel";

const file:Buffer={projectId:"p",path:"src/a.ts",content:"a\r\n😀 old\r\nz",saved:"a\r\n😀 old\r\nz",sha256:"one"};
const proposal:EditProposal={projectId:"p",path:file.path,base:file.content,sha256:"one",start:6,end:9,replacement:"new"};
describe("editor AI changes",()=>{
  it("applies precisely to UTF-16 offsets and preserves CRLF outside the selection",()=>{
    expect(applyProposal(file,proposal)).toBe("a\r\n😀 new\r\nz");
    expect(applyProposal(file,{...proposal,replacement:""})).toBe("a\r\n😀 \r\nz");
  });
  it("rejects edits when the buffer, disk, file or project changed",()=>{
    for(const change of [{content:"user edit"},{sha256:"two"},{path:"other"},{projectId:"other"},{disk:{path:file.path,content:"external",sha256:"two"}}]) expect(()=>applyProposal({...file,...change},proposal)).toThrow();
  });
  it("references unsaved content with line ranges and immutable text",()=>{
    const ref=fileReference({...file,content:"changed",saved:"original"},{start:0,end:7,text:"changed",startLine:2,endLine:2});
    expect(ref.label).toBe("src/a.ts · L2–L2");
    expect(ref.source).toBe("未保存内容快照");
    expect(ref.text).toContain("changed");
    expect(ref.text).not.toContain("original");
  });
  it("diffs reconstruct both inputs, including blank lines and terminal newlines",()=>{
    for(const [a,b] of [["a\nb\nc\n","a\nx\nc\n"],["","\n"],["a\n","a"],["\r\n😀\n","\n😀\r\n"],["one\n".repeat(800),"two\n".repeat(800)]]) {
      const rows=lineDiff(a,b);
      expect(rows.filter(r=>r.kind!=="add").map(r=>r.text).join("")).toBe(a);
      expect(rows.filter(r=>r.kind!=="remove").map(r=>r.text).join("")).toBe(b);
    }
  });
});
