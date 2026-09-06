import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ElapsedTime, elapsedBetween, formatElapsed } from "./ElapsedTime";

describe("formatElapsed", () => {
  it("formats elapsed milliseconds as MM:SS", () => {
    expect(formatElapsed(0)).toBe("00:00");
    expect(formatElapsed(12_000)).toBe("00:12");
    expect(formatElapsed(72_500)).toBe("01:12");
    expect(formatElapsed(3_723_000)).toBe("62:03");
  });

  it("clamps negative and partial-second values", () => {
    expect(formatElapsed(-5_000)).toBe("00:00");
    expect(formatElapsed(999)).toBe("00:00");
    expect(formatElapsed(1_000)).toBe("00:01");
  });

  it("derives completed duration from authoritative start and finish timestamps", () => {
    expect(elapsedBetween("2026-09-05T00:00:00Z", "2026-09-05T00:00:12Z")).toBe(12_000);
    expect(elapsedBetween(undefined, "2026-09-05T00:00:12Z")).toBeNull();
    expect(elapsedBetween("2026-09-05T00:00:12Z", "2026-09-05T00:00:00Z")).toBeNull();
    expect(elapsedBetween("not-a-date", "2026-09-05T00:00:12Z")).toBeNull();
  });
});

describe("ElapsedTime", () => {
  it("renders nothing without a usable start time", () => {
    expect(renderToStaticMarkup(createElement(ElapsedTime, { startedAt: undefined }))).toBe("");
    expect(renderToStaticMarkup(createElement(ElapsedTime, { startedAt: "not-a-date" }))).toBe("");
  });

  it("derives the current elapsed value from turn.startedAt", () => {
    const startedAt = new Date(Date.now() - 5_300).toISOString();
    const html = renderToStaticMarkup(createElement(ElapsedTime, { startedAt }));
    expect(html).toContain('class="work-process-timer"');
    expect(html).toContain("00:05");
  });
});
