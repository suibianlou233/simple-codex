import type { DesktopBridge, PermissionLevel } from "./bridge/types";

const TITLE_LIMIT = 36;

export function buildConversationTitle(content: string): string {
  const normalized = content
    .replace(/[\u0000-\u001f\u007f]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!normalized) return "新对话";
  const characters = Array.from(normalized);
  return characters.length > TITLE_LIMIT
    ? `${characters.slice(0, TITLE_LIMIT).join("")}…`
    : normalized;
}

export async function sendConversationMessage(
  bridge: DesktopBridge,
  projectId: string,
  taskId: string | undefined,
  content: string,
  permissionLevel: PermissionLevel,
): Promise<"started" | "continued"> {
  if (taskId) {
    await bridge.sendMessage(taskId, content);
    return "continued";
  }
  await bridge.startChat(projectId, content, permissionLevel);
  return "started";
}
