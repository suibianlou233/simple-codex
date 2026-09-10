import { describe, expect, it } from "vitest";
import { buildComposerMessage, normalizeAttachments, validateImageFiles } from "./attachmentDraft";
const image = (index: number) => ({path:`simple-image:${index.toString(16).padStart(64,"0")}`,name:`image${index}.png`,sizeBytes:1});
describe("attachment draft invariants", () => {
  it("deduplicates before enforcing the combined draft limit without mutation", () => {
    const items = Array.from({length:8},(_,i)=>image(i));
    expect(normalizeAttachments([...items,image(0)])).toEqual(items);
    expect(()=>normalizeAttachments([...items,image(9)])).toThrow("最多 8");
    expect(items).toHaveLength(8);
  });
  it("sends only surviving attachments, ahead of an unfinished code fence", () => {
    const message = buildComposerMessage("```\ncode", [image(1)]);
    expect(message.indexOf(image(1).path)).toBeLessThan(message.indexOf("```"));
    expect(buildComposerMessage("text", [])).not.toContain("simple-image:");
    expect(buildComposerMessage("", [image(1)])).toContain(image(1).path);
  });
  it("validates the whole batch before import", () => {
    const valid = {name:"ok.png",type:"image/png",size:1} as File;
    expect(()=>validateImageFiles([valid,{name:"bad.txt",type:"text/plain",size:1} as File])).toThrow();
    expect(()=>validateImageFiles([{...valid,size:4*1024*1024+1} as File])).toThrow();
  });
});
