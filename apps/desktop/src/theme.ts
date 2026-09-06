export const THEME_STORAGE_KEY = "local-agent.theme";

export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

type StorageLike = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function browserStorage(): StorageLike | undefined {
  if (typeof window === "undefined") return undefined;
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

export function readThemePreference(
  storage: StorageLike | undefined = browserStorage(),
): ThemePreference {
  try {
    const stored = storage?.getItem(THEME_STORAGE_KEY);
    return stored === "light" || stored === "dark" ? stored : "system";
  } catch {
    return "system";
  }
}

export function persistThemePreference(
  preference: ThemePreference,
  storage: StorageLike | undefined = browserStorage(),
): void {
  try {
    if (preference === "system") {
      storage?.removeItem(THEME_STORAGE_KEY);
    } else {
      storage?.setItem(THEME_STORAGE_KEY, preference);
    }
  } catch {
    // A locked-down WebView may deny storage. The in-memory choice still applies.
  }
}

export function systemPrefersDark(): boolean {
  return typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function resolveTheme(
  preference: ThemePreference,
  prefersDark = systemPrefersDark(),
): ResolvedTheme {
  return preference === "system" ? (prefersDark ? "dark" : "light") : preference;
}

export function applyResolvedTheme(
  theme: ResolvedTheme,
  root: HTMLElement | undefined = typeof document === "undefined"
    ? undefined
    : document.documentElement,
): void {
  if (!root) return;
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
}

export function applyInitialTheme(): ResolvedTheme {
  const theme = resolveTheme(readThemePreference());
  applyResolvedTheme(theme);
  return theme;
}
