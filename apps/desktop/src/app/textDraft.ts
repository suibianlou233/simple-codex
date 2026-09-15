// Keep the complete text in the owning conversation draft, never in a file
// reference or a display placeholder that could accidentally reach the model.
export const LARGE_PASTE_CHAR_THRESHOLD = 1000;

export type PastedTextBlock = { id: string; text: string };
export type TextDraft = { content: string; pastes: PastedTextBlock[] };

export function pasteIntoDraft(draft: TextDraft, text: string, start: number, end: number, id: string): TextDraft {
  if (Array.from(text).length <= LARGE_PASTE_CHAR_THRESHOLD) {
    return { ...draft, content: draft.content.slice(0, start) + text + draft.content.slice(end) };
  }
  // Freeze the prefix with the paste to preserve its exact position. Only the
  // trailing editable text stays in the input; there are no textual placeholders.
  return { content: draft.content.slice(end), pastes: [...draft.pastes,
    { id, text: draft.content.slice(0, start) + text }] };
}

export function composeTextDraft(content: string, pastes: PastedTextBlock[]): string {
  return pastes.map(block => block.text).join("") + content;
}
