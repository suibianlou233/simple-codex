import { describe, expect, it, vi } from "vitest";
import {
  THEME_STORAGE_KEY,
  persistThemePreference,
  readThemePreference,
  resolveTheme,
} from "./theme";

describe("theme preference", () => {
  it("follows the system by default", () => {
    const storage = { getItem: vi.fn(() => null), setItem: vi.fn(), removeItem: vi.fn() };
    expect(readThemePreference(storage)).toBe("system");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("system", true)).toBe("dark");
  });

  it("gives an explicit local choice priority over the system", () => {
    const storage = { getItem: vi.fn(() => "light"), setItem: vi.fn(), removeItem: vi.fn() };
    const preference = readThemePreference(storage);
    expect(resolveTheme(preference, true)).toBe("light");
  });

  it("persists explicit themes and clears storage when returning to system", () => {
    const storage = { getItem: vi.fn(), setItem: vi.fn(), removeItem: vi.fn() };
    persistThemePreference("dark", storage);
    expect(storage.setItem).toHaveBeenCalledWith(THEME_STORAGE_KEY, "dark");
    persistThemePreference("system", storage);
    expect(storage.removeItem).toHaveBeenCalledWith(THEME_STORAGE_KEY);
  });
});
