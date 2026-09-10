import { invoke } from "@tauri-apps/api/core";
import type { AttachmentSummary } from "./types";

export const mediaAvailable = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export type BrowserAction = {
  action: "open" | "read" | "click" | "fill" | "scroll" | "screenshot" | "back" | "forward" | "reload" | "close";
  url?: string; selector?: string; text?: string; delta?: number;
};
export type BrowserResult = { url: string; text?: string; attachment?: AttachmentSummary };
export type BrowserPolicy = { allowHistoryAccess: boolean; denyAll: boolean; deniedOrigins: string[] };
export type BrowserPending = { id: string; taskId: string; origin: string; action: string; url: string; selector?: string; text?: string };
export type BrowserScope = "once" | "turn" | "thread" | "deny";
export type BrowserBounds = {x:number; y:number; width:number; height:number};
let viewportQueue: Promise<unknown> = Promise.resolve();

export const mediaBridge = {
  browserViewport: (projectId: string, bounds: BrowserBounds | null) => {
    const result = viewportQueue.catch(() => undefined).then(() => invoke<void>("browser_viewport", {projectId,bounds}));
    viewportQueue = result;
    return result;
  },
  browserStatus: (projectId: string) => invoke<string | null>("browser_status", {projectId}),
  browserPending: () => invoke<BrowserPending[]>("browser_access_pending"),
  browserResolve: (id: string, scope: BrowserScope) => invoke<void>("browser_access_resolve", { id, scope }),
  browserPolicy: (projectId: string, policy?: BrowserPolicy, revoke = false) => invoke<{policy: BrowserPolicy; grants: number}>("browser_access_policy", {projectId, policy: policy ?? null, revoke}),
  pickImages: (projectId: string) => invoke<AttachmentSummary[]>("pick_images", { projectId }),
  readImage: (projectId: string, reference: string) => invoke<string>("read_image_attachment", { projectId, reference }),
  async pasteImage(projectId: string, file: File): Promise<AttachmentSummary> {
    if (!/\.(png|jpe?g|gif|webp)$/i.test(file.name) && !/^image\/(png|jpeg|gif|webp)$/i.test(file.type)) throw new Error("请拖入 PNG、JPG、GIF 或 WebP 图片；项目文件请使用加号添加");
    if (file.size > 4 * 1024 * 1024) throw new Error("每张图片最多 4 MiB");
    const data = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => typeof reader.result === "string" ? resolve(reader.result.slice(reader.result.indexOf(",") + 1)) : reject(new Error("无法读取图片"));
      reader.onerror = () => reject(new Error("无法读取图片"));
      reader.readAsDataURL(file);
    });
    return invoke<AttachmentSummary>("paste_image", { projectId, name: file.name || "粘贴的图片.png", data });
  },
  browser: (projectId: string, request: BrowserAction) => invoke<BrowserResult>("browser_command", { projectId, request }),
};
