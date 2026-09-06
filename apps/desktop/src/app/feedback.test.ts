import { describe, expect, it } from "vitest";
import { toMessage } from "./feedback";
describe("user-facing error copy", () => {
  it.each([
    ["memory source exclusion saved; artifact cleanup incomplete: private secret-value", "旧对话的自动学习已停止"],
    ["failed to reset memory under C:/private: memory workspace busy; retry after memory processing finishes", "正在整理，尚未清空"],
    ["502 Bad Gateway http://127.0.0.1:8533/private", "暂时无法连接"],
    ["ETIMEDOUT timed out at internal.ts:23", "时间较长"],
    ["401 invalid_api_key secret-value", "未获授权"],
    ["附件 C:/private/file failed", "重新选择"],
    ["Permission denied C:/private", "当前权限"],
    ["TypeError at private.ts:400 secret-value", "这次操作未能完成"],
  ])("maps failures to curated text", (raw, expected) => {
    const text = toMessage(new Error(raw));
    expect(text).toContain(expected);
    expect(text).not.toContain(raw);
    expect(text).not.toContain("secret-value");
    expect(text).not.toContain("private");
    expect(text).not.toContain("http");
  });
});
