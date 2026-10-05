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
