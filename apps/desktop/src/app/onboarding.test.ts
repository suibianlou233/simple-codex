import { afterEach, describe, expect, it, vi } from "vitest";
import { GUIDE_SEEN_KEY, markGuideSeen, shouldShowGuide } from "./onboarding";

afterEach(() => vi.unstubAllGlobals());

describe("first-run guide preference", () => {
  it("shows once after an explicit dismissal and stores no configuration", () => {
    const values = new Map<string, string>();
    vi.stubGlobal("window", { localStorage: {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    } });
    expect(shouldShowGuide()).toBe(true);
    markGuideSeen();
    expect(shouldShowGuide()).toBe(false);
    expect([...values]).toEqual([[GUIDE_SEEN_KEY, "seen"]]);
  });

  it("does not throw when preference storage is blocked", () => {
    vi.stubGlobal("window", { get localStorage() { throw new Error("blocked"); } });
    expect(shouldShowGuide()).toBe(true);
    expect(() => markGuideSeen()).not.toThrow();
  });

  it("supports server rendering without browser globals", () => {
    vi.stubGlobal("window", undefined);
    expect(shouldShowGuide()).toBe(false);
    expect(() => markGuideSeen()).not.toThrow();
  });
});
