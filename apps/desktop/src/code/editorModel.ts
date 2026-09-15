export type EditorFile = { path: string; content: string; sha256: string };
export type Buffer = EditorFile & { projectId: string; saved: string; disk?: EditorFile };
export function reconcile(file: Buffer, disk: EditorFile): Buffer {
  if (disk.sha256 === file.sha256) return {...file, disk: undefined};
  if (file.content !== file.saved) return {...file, disk};
  return {...file, ...disk, saved: disk.content, disk: undefined};
}
export function projectRelativePath(href: string, root: string): string | undefined {
  let path: string;
  try { path = decodeURIComponent(href).replace(/\\/g, "/").replace(/#L\d+.*$/, "").replace(/:\d+(?::\d+)?$/, ""); } catch { return; }
  const base = root.replace(/\\/g,"/").replace(/\/$/,"");
  if (path.toLowerCase().startsWith(base.toLowerCase()+"/")) path = path.slice(base.length+1);
  if (/^\w+:|^\//.test(path) || path.split("/").some(p=>p==="..")) return;
  path = path.replace(/^\.\//,"");
  return path && path !== "." ? path : undefined;
}
