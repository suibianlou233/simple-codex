import { composeTextDraft, LARGE_PASTE_CHAR_THRESHOLD, type PastedTextBlock } from "./textDraft";

export type TextRange = { start: number; end: number };
const cache = new Map<string, TextRange[]>();
async function key(text: string) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return "simple.sent-text.v1." + Array.from(new Uint8Array(digest), x => x.toString(16).padStart(2,"0")).join("");
}
export function pasteRanges(message: string, content: string, pastes: PastedTextBlock[]): TextRange[] {
  const raw = composeTextDraft(content, pastes);
  const trimmed = raw.trim();
  if (!trimmed) return [];
  const offset = message.lastIndexOf(trimmed);
  if (offset < 0) return [];
  const leading = raw.length - raw.trimStart().length;
  let position = 0;
  return pastes.flatMap(block => {
    const start = offset + Math.max(0, position - leading);
    position += block.text.length;
    const end = offset + Math.min(trimmed.length, Math.max(0, position - leading));
    return end > start ? [{start,end}] : [];
  });
}
export async function rememberSentText(message: string, content: string, pastes: PastedTextBlock[]) {
  if (!pastes.length) return;
  const ranges = pasteRanges(message, content, pastes);
  cache.set(message, ranges);
  if (cache.size > 32) cache.delete(cache.keys().next().value!);
  // Store only presentation offsets, never another copy of the user's document.
  try { localStorage.setItem(await key(message), JSON.stringify(ranges)); } catch { /* Sending must still work when storage is unavailable. */ }
}
export async function sentTextRanges(message: string): Promise<TextRange[]> {
  const cached = cache.get(message);
  if (cached) return cached;
  try {
    const value: unknown = JSON.parse(localStorage.getItem(await key(message)) ?? "null");
    let previous = 0;
    if (Array.isArray(value) && value.length && value.every(r => {
      if (!r || !Number.isInteger(r.start) || !Number.isInteger(r.end) || r.start < previous || r.end <= r.start || r.end > message.length) return false;
      previous = r.end;
      return true;
    })) return value;
  } catch { /* Older messages still get a compact whole-document card. */ }
  return fallbackTextRanges(message);
}
export function fallbackTextRanges(message: string): TextRange[] {
  return message.length > LARGE_PASTE_CHAR_THRESHOLD && Array.from(message).length > LARGE_PASTE_CHAR_THRESHOLD
    ? [{start:0,end:message.length}] : [];
}
