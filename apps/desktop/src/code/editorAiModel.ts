import type { Buffer } from "./editorModel";
import type { PastedTextBlock } from "../app/textDraft";

export type EditorSelection = { start: number; end: number; text: string; startLine: number; endLine: number };
export type EditProposal = { projectId: string; path: string; base: string; sha256: string; start: number; end: number; replacement: string };

export function applyProposal(file: Buffer, proposal: EditProposal): string {
  if (file.projectId !== proposal.projectId || file.path !== proposal.path || file.content !== proposal.base || file.sha256 !== proposal.sha256 || file.disk) {
    throw new Error("文件或编辑内容已变化，建议未应用。请重新选择代码并生成建议。");
  }
  if (proposal.start < 0 || proposal.end > file.content.length || proposal.start >= proposal.end) throw new Error("选区已失效");
  return file.content.slice(0, proposal.start) + proposal.replacement + file.content.slice(proposal.end);
}

export function fileReference(file: Buffer, selection?: EditorSelection): PastedTextBlock {
  const selected = selection && selection.end > selection.start ? selection : undefined;
  const lineLabel = selected ? `L${selected.startLine}–L${selected.endLine}` : "全文";
  const dirty = file.content !== file.saved;
  const body = selected ? file.content.slice(selected.start, selected.end) : file.content;
  return {
    id: crypto.randomUUID(), label: `${file.path} · ${lineLabel}`,
    source: dirty ? "未保存内容快照" : "文件内容快照",
    text: `项目文件：${file.path}\n范围：${lineLabel}\n来源：${dirty ? "编辑器未保存内容" : "文件内容"}（添加时的快照）\n\n${body}`,
  };
}

export type DiffRow = { kind: "same" | "add" | "remove"; text: string; oldLine?: number; newLine?: number };
// Bounded LCS: small edits get exact line alignment, large rewrites remain responsive.
export function lineDiff(before: string, after: string): DiffRow[] {
  const lines = (s: string) => s.match(/[^\n]*\n|[^\n]+$/g) ?? [];
  const a = lines(before), b = lines(after), rows: DiffRow[] = [];
  let prefix = 0, suffix = 0;
  while (prefix < a.length && prefix < b.length && a[prefix] === b[prefix]) prefix++;
  while (suffix < a.length-prefix && suffix < b.length-prefix && a[a.length-1-suffix] === b[b.length-1-suffix]) suffix++;
  let oldLine = 1, newLine = 1;
  const emit = (kind: DiffRow["kind"], text: string) => rows.push({kind, text, oldLine:kind!=="add"?oldLine++:undefined,newLine:kind!=="remove"?newLine++:undefined});
  for (let i=0;i<prefix;i++) emit("same",a[i]);
  const x = a.slice(prefix,a.length-suffix), y = b.slice(prefix,b.length-suffix);
  if (x.length*y.length <= 500_000) {
    const width=y.length+1, table=new Uint32Array((x.length+1)*width);
    for(let i=x.length-1;i>=0;i--) for(let j=y.length-1;j>=0;j--) table[i*width+j]=x[i]===y[j]?1+table[(i+1)*width+j+1]:Math.max(table[(i+1)*width+j],table[i*width+j+1]);
    let i=0,j=0;
    while(i<x.length || j<y.length) {
      if(i<x.length && j<y.length && x[i]===y[j]) {emit("same",x[i++]);j++;}
      else if(i<x.length && (j===y.length || table[(i+1)*width+j]>=table[i*width+j+1])) emit("remove",x[i++]);
      else emit("add",y[j++]);
    }
  } else { x.forEach(line=>emit("remove",line));y.forEach(line=>emit("add",line)); }
  for(let i=a.length-suffix;i<a.length;i++) emit("same",a[i]);
  return rows;
}

export type EditHunk={before:string;after:string;line:number};
export function proposalHunks(before:string,after:string):{parts:(string|EditHunk)[];hunks:EditHunk[]} {
 const parts:(string|EditHunk)[]=[],hunks:EditHunk[]=[];let current:EditHunk|undefined,line=1;
 for(const row of lineDiff(before,after)) {if(row.kind==="same"){current=undefined;parts.push(row.text);line++;}else{if(!current){current={before:"",after:"",line};parts.push(current);hunks.push(current);}if(row.kind==="remove"){current.before+=row.text;line++;}else current.after+=row.text;}}
 return {parts,hunks};
}
export function resolveHunks(diff:ReturnType<typeof proposalHunks>,accepted:Set<number>):string{return diff.parts.map(p=>typeof p==="string"?p:accepted.has(diff.hunks.indexOf(p))?p.after:p.before).join("");}
