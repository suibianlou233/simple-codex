import { useEffect, useState } from "react";

export function formatElapsed(milliseconds: number): string {
  const totalSeconds = Math.max(0, Math.floor(milliseconds / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

export function elapsedBetween(
  startedAt: string | null | undefined,
  finishedAt: string | null | undefined,
): number | null {
  if (!startedAt || !finishedAt) return null;
  const startMs = Date.parse(startedAt);
  const finishMs = Date.parse(finishedAt);
  if (!Number.isFinite(startMs) || !Number.isFinite(finishMs)) return null;
  return finishMs >= startMs ? finishMs - startMs : null;
}

export function ElapsedTime({ startedAt }: { startedAt?: string | null }) {
  const startMs = startedAt ? Date.parse(startedAt) : Number.NaN;
  const tracking = Number.isFinite(startMs);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!tracking) return undefined;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [tracking]);

  if (!tracking) return null;
  return <span className="work-process-timer">{formatElapsed(now - startMs)}</span>;
}
