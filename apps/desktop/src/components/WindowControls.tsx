import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

export function WindowControls({onError}:{onError:(message:string)=>void}) {
  const native = typeof window!=="undefined" && "__TAURI_INTERNALS__" in window;
  const [maximized,setMaximized] = useState(false);
  useEffect(()=>{
    if(!native)return;
    const appWindow=getCurrentWindow();
    let disposed=false;let unlisten:(()=>void)|undefined;
    const refresh=()=>void appWindow.isMaximized().then(value=>{if(!disposed)setMaximized(value);}).catch(()=>{});
    refresh();
    void appWindow.onResized(refresh).then(stop=>{if(disposed)stop();else unlisten=stop;}).catch(()=>{});
    return()=>{disposed=true;unlisten?.();};
  },[native]);
  const perform=async(action:"minimize"|"maximize"|"close")=>{
    if(!native)return;
    try {
      const appWindow=getCurrentWindow();
      if(action==="minimize")await appWindow.minimize();
      else if(action==="maximize"){await appWindow.toggleMaximize();setMaximized(await appWindow.isMaximized());}
      // close() preserves the editor's unsaved-work confirmation. Never destroy here.
      else await appWindow.close();
    }catch(error){onError(`窗口操作失败：${String(error)}`);}
  };
  return <div className="window-controls" aria-label="窗口控制">
    <button type="button" disabled={!native} aria-label="最小化窗口" title="最小化" onClick={()=>void perform("minimize")}><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M1 6.5h10"/></svg></button>
    <button type="button" disabled={!native} aria-label={maximized?"还原窗口":"最大化窗口"} title={maximized?"还原":"最大化"} onClick={()=>void perform("maximize")}><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">{maximized?<path d="M3.5 3.5v-2h7v7h-2m-7-5h7v7h-7z"/>:<rect x="1.5" y="1.5" width="9" height="9"/>}</svg></button>
    <button type="button" className="window-close" disabled={!native} aria-label="关闭窗口" title="关闭" onClick={()=>void perform("close")}><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="m1.5 1.5 9 9m0-9-9 9"/></svg></button>
  </div>;
}
