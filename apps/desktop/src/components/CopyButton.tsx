import { useEffect, useRef, useState } from "react";
import { Icon } from "./Icon";

export function CopyButton({ text, label = "复制消息" }: { text: string; label?: string }) {
  const [state, setState] = useState("复制");
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const copy = async () => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("Clipboard unavailable");
      await navigator.clipboard.writeText(text);
      setState("已复制");
    } catch {
      setState("复制失败，请手动选择");
    }
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setState("复制"), 2500);
  };
  return <button type="button" aria-label={label} title={state} onClick={() => void copy()}><Icon name={state === "已复制" ? "check" : "copy"} size={13} /><span aria-live="polite">{state}</span></button>;
}
