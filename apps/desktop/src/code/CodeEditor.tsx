import { useEffect, useRef } from "react";
import { basicSetup } from "codemirror";
import { EditorView, keymap } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";
import { markdown } from "@codemirror/lang-markdown";

function language(path: string) {
  if (/\.[cm]?[jt]sx?$/.test(path)) return javascript({typescript: /tsx?$/.test(path), jsx: /[jt]sx$/.test(path)});
  if (/\.json$/.test(path)) return json();
  if (/\.py$/.test(path)) return python();
  if (/\.rs$/.test(path)) return rust();
  if (/\.html?$/.test(path)) return html();
  if (/\.css$/.test(path)) return css();
  if (/\.md$/.test(path)) return markdown();
  return [];
}
export function CodeEditor(props: { path: string; content: string; dark: boolean; readOnly?: boolean; onChange?: (value:string)=>void; onSave?:()=>void; onSelection?:(value:string)=>void }) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const latest = useRef(props); latest.current = props;
  const external = useRef(false);
  useEffect(()=> {
    const editor = new EditorView({parent:host.current!,state:EditorState.create({doc:latest.current.content, extensions:[
      basicSetup, language(props.path), EditorState.readOnly.of(Boolean(props.readOnly)),
      EditorState.lineSeparator.of(latest.current.content.includes("\r\n") ? "\r\n" : "\n"),
      EditorView.theme({"&":{height:"100%",background:"var(--surface)",color:"var(--text)"},".cm-scroller":{overflow:"auto",fontFamily:"Consolas, Menlo, monospace",fontSize:"13px"},".cm-gutters":{background:"var(--surface-subtle)",color:"var(--text-tertiary)",border:"none"},".cm-content":{caretColor:"var(--text)"}}, {dark:props.dark}),
      keymap.of([{key:"Mod-s",run:()=>{latest.current.onSave?.();return true;}}]),
      EditorView.updateListener.of(update=> {
        if (update.docChanged && !external.current) latest.current.onChange?.(update.state.doc.toString());
        if(update.selectionSet || update.docChanged) { const range=update.state.selection.main; latest.current.onSelection?.(update.state.sliceDoc(range.from,range.to)); }
      }),
    ]})});
    view.current=editor;
    return ()=>{editor.destroy();view.current=null;};
  },[props.path,props.dark,props.readOnly]);
  useEffect(()=>{const editor=view.current;if(editor && editor.state.doc.toString()!==props.content){external.current=true;editor.dispatch({changes:{from:0,to:editor.state.doc.length,insert:props.content}});external.current=false;}},[props.content]);
  return <div className="code-editor-host" ref={host}/>;
}
