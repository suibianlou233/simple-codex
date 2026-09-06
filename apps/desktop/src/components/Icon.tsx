import type { CSSProperties } from "react";

export type IconName = "sidebar" | "plus" | "search" | "folder" | "chat" | "chevron" | "settings" | "terminal" | "diff" | "review" | "close" | "arrow" | "paperclip" | "code" | "check" | "copy" | "spark" | "more";
const paths: Record<IconName, string> = {
  sidebar: "M4 4h16v16H4z M9 4v16",
  plus: "M12 5v14 M5 12h14",
  search: "M16 16l5 5 M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0",
  folder: "M3 7V5h6l2 3h10v12H3z",
  chat: "M4 4h16v13H9l-5 4z",
  chevron: "m9 5 7 7-7 7",
  settings: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8 M12 2v3 M12 19v3 M2 12h3 M19 12h3 M5 5l2 2 M17 17l2 2 M5 19l2-2 M17 7l2-2",
  terminal: "m5 7 5 5-5 5 M13 17h6",
  diff: "M5 3h10l4 4v14H5z M15 3v5h4 M8 12h8 M12 9v6 M8 18h8",
  review: "M12 3 4 6v6c0 5 8 9 8 9s8-4 8-9V6z m-4 9 3 3 5-6",
  close: "m6 6 12 12 M6 18 18 6",
  arrow: "M12 19V5 m-6 6 6-6 6 6",
  paperclip: "m8 13 7-7a3 3 0 0 1 4 4L9 20a5 5 0 0 1-7-7L13 2 M5 16l10-10",
  code: "m8 6-6 6 6 6 M16 6l6 6-6 6 M14 3l-4 18",
  check: "m5 12 4 4L19 6",
  copy: "M8 8h13v13H8z M16 8V3H3v13h5",
  spark: "m12 2 3 7 7 3-7 3-3 7-3-7-7-3 7-3z",
  more: "M5 12h.01 M12 12h.01 M19 12h.01",
};

export function Icon({ name, size = 18, style }: { name: IconName; size?: number; style?: CSSProperties }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={name === "more" ? 3 : 1.6} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}><path d={paths[name]} /></svg>;
}

export function SimpleMark({ large = false }: { large?: boolean }) {
  return <span className={`simple-symbol${large ? " simple-symbol-large" : ""}`} aria-hidden="true"><svg viewBox="0 0 32 32" fill="none"><path d="M23 7H13a6 6 0 0 0 0 12h6a3 3 0 0 1 0 6H9 M9 25h10a6 6 0 0 0 0-12h-6a3 3 0 0 1 0-6h10" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" /></svg></span>;
}
