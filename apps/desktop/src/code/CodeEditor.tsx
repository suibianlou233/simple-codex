import {useEffect, useRef, useState} from "react";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api";
import "monaco-editor/esm/vs/language/json/monaco.contribution";
import JsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import "monaco-editor/esm/vs/basic-languages/typescript/typescript.contribution";
import "monaco-editor/esm/vs/basic-languages/javascript/javascript.contribution";
import "monaco-editor/esm/vs/basic-languages/python/python.contribution";
import "monaco-editor/esm/vs/basic-languages/rust/rust.contribution";
import "monaco-editor/esm/vs/basic-languages/html/html.contribution";
import "monaco-editor/esm/vs/basic-languages/css/css.contribution";
import "monaco-editor/esm/vs/basic-languages/markdown/markdown.contribution";
import {queryLanguage} from "./languageService";
import type {Buffer} from "./editorModel";
import type {Location} from "./ProjectSearch";
import type {EditorSelection} from "./editorAiModel";
(self as typeof self & {MonacoEnvironment: monaco.Environment}).MonacoEnvironment = {getWorker: (_moduleId, label) => label === "json" ? new JsonWorker() : new EditorWorker()};
const language = (path: string) => ({ts:"typescript",tsx:"typescript",js:"javascript",jsx:"javascript",mjs:"javascript",cjs:"javascript",mts:"typescript",cts:"typescript",json:"json",py:"python",rs:"rust",html:"html",css:"css",md:"markdown"}[path.split(".").pop() ?? ""] ?? "plaintext");
export function CodeEditor(props: { navigation?:{kind:"definition"|"references";nonce:number};projectId?:string;buffers?:Record<string,Buffer>;onNavigate?:(location:Location)=>void;initialCaret?:{line:number;column:number};reveal?:{line:number;column?:number;nonce:number};onCaret?:(caret:{line:number;column:number})=>void;path: string; content: string; dark: boolean; readOnly?: boolean; onChange?: (value:string)=>void; onSave?:()=>void; onSelection?:(value:EditorSelection)=>void; onInlineEdit?:()=>void }) {
 const host = useRef<HTMLDivElement>(null), view = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
 const latest = useRef(props); latest.current = props;
 const external = useRef(false);
 const [locations,setLocations] = useState<Location[]>([]), [navigationNote,setNavigationNote] = useState("");
 async function navigate(kind: "definition" | "references") {
  const editor=view.current,p=latest.current,model=editor?.getModel(),position=editor?.getPosition();
  if(!editor||!model||!position||!p.projectId)return;
  setNavigationNote("分析中…");
  try {const result=await queryLanguage(p.projectId,p.path,model.getOffsetAt(position),kind,p.buffers??{});
   if(view.current!==editor)return;
   setLocations(result.locations??[]);setNavigationNote(result.limited?"项目超过索引上限，结果可能不完整":result.locations?.length?"":"未找到项目内的结果（第三方包和路径别名暂不索引）");
   if(kind==="definition"&&result.locations?.length===1)p.onNavigate?.(result.locations[0]);
  } catch(error) {if(view.current===editor)setNavigationNote(String(error));}
 }
 useEffect(() => {
  const model=monaco.editor.createModel(latest.current.content, language(props.path));
  const editor=monaco.editor.create(host.current!, {model, automaticLayout:true, theme:latest.current.dark?"vs-dark":"vs", readOnly:props.readOnly, fontSize:13,fontFamily:"Consolas, Menlo, monospace",renderWhitespace:"none",minimap:{enabled:false},scrollBeyondLastLine:false, padding:{top:12},ariaLabel:"代码编辑器", contextmenu:false});
  view.current=editor;
  const emitSelection=()=>{const selection=editor.getSelection();if(!selection)return;const from=model.getOffsetAt(selection.getStartPosition()),to=model.getOffsetAt(selection.getEndPosition());latest.current.onSelection?.({start:from,end:to,text:model.getValueInRange(selection),startLine:selection.startLineNumber,endLine:model.getPositionAt(Math.max(from,to-1)).lineNumber});const pos=editor.getPosition();if(pos)latest.current.onCaret?.({line:pos.lineNumber,column:pos.column});};
  const change=editor.onDidChangeModelContent(()=>{if(!external.current)latest.current.onChange?.(model.getValue());emitSelection();});
  const selection=editor.onDidChangeCursorSelection(emitSelection);
  editor.addCommand(monaco.KeyMod.CtrlCmd|monaco.KeyCode.KeyS,()=>latest.current.onSave?.());
  editor.addCommand(monaco.KeyMod.CtrlCmd|monaco.KeyCode.KeyK,()=>latest.current.onInlineEdit?.());
  editor.addCommand(monaco.KeyCode.F12,()=>void navigate("definition"));
  editor.addCommand(monaco.KeyMod.Shift|monaco.KeyCode.F12,()=>void navigate("references"));
  const hover=monaco.languages.registerHoverProvider(language(props.path),{provideHover:async(candidate,position,token)=>{const p=latest.current;if(candidate!==model||!p.projectId)return null;try{const result=await queryLanguage(p.projectId,p.path,model.getOffsetAt(position),"hover",p.buffers??{});return token.isCancellationRequested||!result.hover?null:{contents:[{value:"```typescript\n"+result.hover+"\n```"}]};}catch{return null;}}});
  const caret=latest.current.initialCaret;if(caret){const position=model.validatePosition({lineNumber:caret.line,column:caret.column});editor.setPosition(position);editor.revealPositionInCenter(position);}
  return ()=>{hover.dispose();change.dispose();selection.dispose();editor.dispose();model.dispose();view.current=null;};
 },[props.path,props.projectId,props.readOnly]);
 useEffect(()=>{monaco.editor.setTheme(props.dark?"vs-dark":"vs");},[props.dark]);
 useEffect(()=>{const editor=view.current,model=editor?.getModel();if(!editor||!model||model.getValue()===props.content)return;external.current=true;try{editor.executeEdits("external",[{range:model.getFullModelRange(),text:props.content,forceMoveMarkers:true}]);}finally{external.current=false;}},[props.content]);
 useEffect(()=>{const editor=view.current,r=props.reveal,model=editor?.getModel();if(!editor||!model||!r)return;const position=model.validatePosition({lineNumber:r.line,column:r.column??1});editor.setPosition(position);editor.revealPositionInCenter(position);editor.focus();},[props.reveal]);
 useEffect(()=>{if(props.navigation)void navigate(props.navigation.kind);},[props.navigation]);
 return <>{navigationNote||locations.length?<div className="code-navigation"><small>{navigationNote||`${locations.length} 个位置`}</small><button aria-label="关闭导航结果" onClick={()=>{setLocations([]);setNavigationNote("");}}>×</button></div>:null}{locations.length?<div className="code-locations">{locations.map((loc,i)=><button key={i} onClick={()=>props.onNavigate?.(loc)}>{loc.path}:{loc.line}:{loc.column}</button>)}</div>:null}<div className="code-editor-host" ref={host}/></>;
}
