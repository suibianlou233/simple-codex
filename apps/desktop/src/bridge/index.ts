import { desktopBridge as memoryDesktopBridge } from "./memoryBridge";
import { tauriDesktopBridge } from "./tauriBridge";

const runningInTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const desktopBridge = runningInTauri
  ? tauriDesktopBridge
  : memoryDesktopBridge;
