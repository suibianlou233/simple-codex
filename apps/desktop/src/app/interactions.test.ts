import { describe, expect, it } from "vitest";
import { composerHeight, emptyConversationLabel, isComposingKey, shouldSubmitMessage, toggleInspector } from "./interactions";

describe("composer interaction", () => {
  const enter = { key: "Enter", shiftKey: false, isComposing: false, keyCode: 13 };
  it("submits ordinary Enter but never IME confirmation or Shift+Enter", () => {
    expect(shouldSubmitMessage(enter)).toBe(true);
    expect(shouldSubmitMessage({ ...enter, isComposing: true })).toBe(false);
    expect(shouldSubmitMessage({ ...enter, keyCode: 229 })).toBe(false);
    expect(shouldSubmitMessage({ ...enter, shiftKey: true })).toBe(false);
    expect(shouldSubmitMessage({ ...enter, key: "a" })).toBe(false);
  });
  it("keeps task search Enter and arrows active outside IME composition", () => {
    expect(isComposingKey({ isComposing: false, keyCode: 13 })).toBe(false);
    expect(isComposingKey({ isComposing: false, keyCode: 40 })).toBe(false);
  });
  it("lets the IME finish candidates on Enter and arrows during composition", () => {
    expect(isComposingKey({ isComposing: true, keyCode: 13 })).toBe(true);
    expect(isComposingKey({ isComposing: true, keyCode: 40 })).toBe(true);
    expect(isComposingKey({ isComposing: false, keyCode: 229 })).toBe(true);
  });
  it("grows for multiline input, caps long content and shrinks after clearing", () => {
    expect(composerHeight(34)).toBe(34);
    expect(composerHeight(98)).toBe(98);
    expect(composerHeight(900)).toBe(180);
    expect(composerHeight(0)).toBe(34);
  });
});

describe("workspace navigation", () => {
  it("opens, switches and closes the inspector with the same control", () => {
    expect(toggleInspector(undefined, "files")).toBe("files");
    expect(toggleInspector("files", "diff")).toBe("diff");
    expect(toggleInspector("diff", "diff")).toBeUndefined();
  });
  it("distinguishes an empty project from an empty search", () => {
    expect(emptyConversationLabel(true, "abc")).toContain("没有找到匹配");
    expect(emptyConversationLabel(true, "  ")).toBe("还没有对话");
    expect(emptyConversationLabel(false, "abc")).toContain("打开项目后");
  });
});
