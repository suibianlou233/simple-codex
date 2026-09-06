export const GUIDE_SEEN_KEY = "simple.ui.onboarding.v1";

export function shouldShowGuide(): boolean {
  try {
    return typeof window !== "undefined" && window.localStorage.getItem(GUIDE_SEEN_KEY) !== "seen";
  } catch {
    return true;
  }
}

export function markGuideSeen(): void {
  try {
    if (typeof window !== "undefined") window.localStorage.setItem(GUIDE_SEEN_KEY, "seen");
  } catch {
    // A disabled/full preference store must not block configuration or dismissal.
  }
}
