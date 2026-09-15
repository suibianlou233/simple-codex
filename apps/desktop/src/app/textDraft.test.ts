import { describe, expect, it } from "vitest";
import { pasteIntoDraft, composeTextDraft } from "./textDraft";
import { buildComposerMessage } from "./attachmentDraft";

describe("long text drafts", () => {
  it("replaces the selection and preserves both surrounding instructions", () => {
    const result = pasteIntoDraft({content:"before OLD after",pastes:[]}, "中😀\r\n".repeat(600), 7, 10, "one");
    expect(composeTextDraft(result.content,result.pastes)).toBe("before " + "中😀\r\n".repeat(600) + " after");
    expect(result.content).toBe(" after");
  });
  it("counts Unicode characters rather than UTF-16 units at the folding boundary", () => {
    expect(pasteIntoDraft({content:"",pastes:[]}, "😀".repeat(1000), 0, 0, "one").pastes).toHaveLength(0);
    expect(pasteIntoDraft({content:"",pastes:[]}, "😀".repeat(1001), 0, 0, "one").pastes).toHaveLength(1);
  });
  it("submits a million characters intact alongside file references", () => {
    const text = "文".repeat(1_000_000);
    const draft = pasteIntoDraft({content:"分析：",pastes:[]}, text, 3, 3, "one");
    const message = buildComposerMessage(composeTextDraft(draft.content,draft.pastes), [{ path: "notes.md", name: "notes.md", sizeBytes: 10 }]);
    expect(message).toContain("notes.md");
    expect(message.endsWith("分析：" + text)).toBe(true);
  });
});
