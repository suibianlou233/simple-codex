import { useMemo, useState } from "react";
import { lineDiff } from "./editorAiModel";

export function DiffView({before,after}:{before:string;after:string}) {
  const rows=useMemo(()=>lineDiff(before,after),[before,after]);
  const [all,setAll]=useState(false);
  const visible = all ? rows : rows.filter((row,i)=>row.kind!=="same" || rows.slice(Math.max(0,i-3),i+4).some(r=>r.kind!=="same"));
  return <section className="code-diff" aria-label="修改差异">
    <header><span className="diff-added">+{rows.filter(r=>r.kind==="add").length}</span><span className="diff-removed">−{rows.filter(r=>r.kind==="remove").length}</span><button onClick={()=>setAll(!all)}>{all?"仅看修改":"显示全文"}</button></header>
    {before===after?<p>内容没有变化</p>:null}
    <div className="code-diff-lines">{visible.slice(0,5000).map((row,i)=><div className={`code-diff-line diff-${row.kind}`} key={i}><span>{row.oldLine??""}</span><span>{row.newLine??""}</span><b>{row.kind==="add"?"+":row.kind==="remove"?"−":" "}</b><code>{row.text.replace(/\r?\n$/,"") || " "}{!row.text.endsWith("\n")?<small> ⏎ 无行末换行</small>:null}</code></div>)}</div>
    {visible.length>5000?<p>预览仅显示前 5000 行，实际应用保留全部内容。</p>:null}
  </section>;
}

export function PatchView({patch}:{patch:string}) {
  return <pre className="code-patch" aria-label="补丁内容">{patch.split("\n").slice(0,5000).map((line,i)=><span key={i} className={line.startsWith("+")&&!line.startsWith("+++")?"diff-add":line.startsWith("-")&&!line.startsWith("---")?"diff-remove":line.startsWith("@@")?"diff-hunk":""}>{line}{"\n"}</span>)}{patch.split("\n").length>5000?"\n预览已截断，请用 Git 查看完整差异。":""}</pre>;
}
