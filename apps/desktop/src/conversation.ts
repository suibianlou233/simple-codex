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
  contextMode?: "simple" | "code",
): Promise<"started" | "continued"> {
  if (taskId) {
    if(contextMode)await bridge.sendMessage(taskId, content, contextMode);else await bridge.sendMessage(taskId,content);
    return "continued";
  }
  if(contextMode)await bridge.startChat(projectId,content,permissionLevel,contextMode);else await bridge.startChat(projectId,content,permissionLevel);
  return "started";
}
