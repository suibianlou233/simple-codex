import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { DesktopBridge } from "../bridge/types";
import "@xterm/xterm/css/xterm.css";

export function TerminalPanel({ taskId, onClose }: { bridge: DesktopBridge; taskId: string; onClose: () => void }) {
  const host = useRef<HTMLDivElement>(null);
  const [cwd, setCwd] = useState("PowerShell");
  const [generation, setGeneration] = useState(0);
  const [ended, setEnded] = useState(false);
  const [error, setError] = useState<string>();
  useEffect(() => {
    setEnded(false);
    setError(undefined);
    let disposed = false;
    let id: string | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let release = () => {};
    const fail = (cause: unknown) => { if (!disposed) setError(String(cause)); };
    void (async () => {
      const [{ Terminal }, { FitAddon }] = await Promise.all([import("@xterm/xterm"), import("@xterm/addon-fit")]);
      if (disposed || !host.current) return;
      const terminal = new Terminal({ cursorBlink: true, fontSize: 13, fontFamily: '"Cascadia Mono", "Cascadia Code", Consolas, monospace', scrollback: 5000, theme: { background: "#171a19", foreground: "#e2e7e4", cursor: "#e2e7e4" } });
      const fit = new FitAddon();
      terminal.loadAddon(fit); terminal.open(host.current);
      const resize = () => { if (host.current && host.current.clientWidth > 0 && host.current.clientHeight > 0) fit.fit(); };
      resize();
      const observer = new ResizeObserver(resize); observer.observe(host.current);
      let writes = Promise.resolve();
      const input = terminal.onData(data => {
        if (!id || disposed) return;
        const session = id;
        for (const chunk of data.match(/.{1,4096}/gsu) ?? []) writes = writes.then(() => invoke<void>("pty_write", { id: session, data: chunk })).catch(fail);
      });
      const dimensions = terminal.onResize(({ cols, rows }) => { if (id) void invoke("pty_resize", { id, cols, rows }).catch(fail); });
      terminal.attachCustomKeyEventHandler(event => {
        if (event.type === "keydown" && event.ctrlKey && event.key.toLowerCase() === "c" && terminal.hasSelection()) {
          void navigator.clipboard.writeText(terminal.getSelection()).catch(fail); return false;
        }
        if (event.type === "keydown" && event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "v") {
          void navigator.clipboard.readText().then(text => { if (!disposed) terminal.paste(text); }).catch(fail); return false;
        }
        return true;
      });
      release = () => { observer.disconnect(); input.dispose(); dimensions.dispose(); terminal.dispose(); };
      if (!isTauri()) { terminal.writeln("请在 Simple 桌面端使用本地交互终端。"); return; }
      const opened = await invoke<{ id: string; cwd: string }>("pty_open", { taskId, cols: terminal.cols, rows: terminal.rows });
      id = opened.id;
      if (disposed) { await invoke("pty_close", { id }); return; }
      setCwd(opened.cwd); resize();
      await invoke("pty_resize", { id, cols: terminal.cols, rows: terminal.rows });
      terminal.focus();
      const read = async () => {
        if (disposed || !id) return;
        try {
          const output = await invoke<{ data: number[]; exited: boolean }>("pty_read", { id });
          if (disposed) return;
          if (output.data.length) await new Promise<void>(resolve => terminal.write(new Uint8Array(output.data), resolve));
          if (disposed) return;
          if (output.exited) { setEnded(true); terminal.writeln("\r\n[终端会话已结束]"); await invoke("pty_close", { id }); id = undefined; return; }
          timer = setTimeout(() => { void read(); }, output.data.length ? 0 : 40);
        } catch (cause) { fail(cause); }
      };
      void read();
    })().catch(fail);
    return () => { disposed = true; clearTimeout(timer); release(); if (id) void invoke("pty_close", { id }).catch(() => {}); };
  }, [taskId, generation]);
  return <section className="terminal-panel" aria-label="集成终端">
    <header><div><strong>终端</strong><small title={cwd}>{cwd}</small></div>{ended ? <button type="button" onClick={() => setGeneration(value => value + 1)}>重新打开</button> : null}<button type="button" aria-label="收起终端" onClick={onClose}>×</button></header>
    <div className="terminal-screen" ref={host} />
    {error ? <p className="panel-error" role="alert">{error}</p> : null}
  </section>;
}
