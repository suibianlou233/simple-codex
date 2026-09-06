export function shouldSubmitMessage(event: {
  key: string;
  shiftKey: boolean;
  isComposing: boolean;
  keyCode: number;
}): boolean {
  // Some IMEs report keyCode 229 on the key that confirms a candidate.
  return event.key === "Enter" && !event.shiftKey && !event.isComposing && event.keyCode !== 229;
}

export function isComposingKey(event: { isComposing: boolean; keyCode: number }): boolean {
  // While an IME candidate is active, Enter and arrow keys belong to the input method.
  return event.isComposing || event.keyCode === 229;
}

export function composerHeight(scrollHeight: number): number {
  return Math.min(180, Math.max(34, scrollHeight));
}

export function toggleInspector<T extends string>(current: T | undefined, next: T): T | undefined {
  return current === next ? undefined : next;
}

export function emptyConversationLabel(hasProject: boolean, query: string): string {
  if (!hasProject) return "打开项目后，对话会出现在这里";
  return query.trim() ? "没有找到匹配的对话，试试其他关键词" : "还没有对话";
}
