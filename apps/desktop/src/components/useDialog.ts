import { useEffect, useRef } from "react";

// Local modal focus: Escape closes; Tab stays inside; closing restores the trigger.
export function useDialog<T extends HTMLElement = HTMLElement>(onClose: () => void) {
  const ref = useRef<T>(null);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const node = ref.current;
    const controls = () => Array.from(node?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex="0"]') ?? []).filter((element) => element.getClientRects().length > 0);
    (controls()[0] ?? node)?.focus();
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close.current(); }
      if (event.key !== "Tab") return;
      const items = controls();
      const index = items.indexOf(document.activeElement as HTMLElement);
      if (!items.length) { event.preventDefault(); node?.focus(); return; }
      if (event.shiftKey && index <= 0) { event.preventDefault(); items.at(-1)?.focus(); }
      else if (!event.shiftKey && (index === items.length - 1 || index < 0)) { event.preventDefault(); items[0].focus(); }
    };
    node?.addEventListener("keydown", keydown);
    return () => { node?.removeEventListener("keydown", keydown); previous?.focus(); };
  }, []);
  return ref;
}
