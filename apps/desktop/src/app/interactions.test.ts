import { describe, expect, it } from "vitest";
import { composerHeight, isComposingKey, shouldSubmitMessage } from "./interactions";

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
