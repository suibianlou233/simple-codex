import { describe, expect, it } from "vitest";
import { pasteRanges, fallbackTextRanges } from "./sentText";
import { buildComposerMessage } from "./attachmentDraft";
describe("sent text presentation",()=>{
  it("keeps exact ranges through attachment prefixes and outer whitespace trimming",()=>{
    const pastes=[{id:"a",text:"  第一段😀\r\n".repeat(300)},{id:"b",text:"第二段".repeat(500)}];
    const raw=pastes.map(p=>p.text).join("")+"\n补充要求  ";
    const message=buildComposerMessage(raw,[{name:"notes.md",path:"notes.md",sizeBytes:1}]);
    const ranges=pasteRanges(message,"\n补充要求  ",pastes);
    expect(message.slice(ranges[0].start,ranges[0].end)).toBe(pastes[0].text.trimStart());
    expect(message.slice(ranges[1].start,ranges[1].end)).toBe(pastes[1].text);
    expect(message.slice(ranges[1].end)).toBe("\n补充要求");
    expect(message.slice(0,ranges[0].start)).toContain("notes.md");
  });
  it("folds old long messages without inventing a body/instruction boundary",()=>{
    expect(fallbackTextRanges("字".repeat(30000))).toEqual([{start:0,end:30000}]);
    expect(fallbackTextRanges("😀".repeat(1000))).toEqual([]);
  });
});
