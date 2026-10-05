import ts from "typescript";
import es5 from "typescript/lib/lib.es5.d.ts?raw";
import dom from "typescript/lib/lib.dom.d.ts?raw";
type File={path:string;content:string};
let files=new Map<string,string>();
const options:ts.CompilerOptions={allowJs:true,checkJs:true,jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext,moduleResolution:ts.ModuleResolutionKind.Bundler,allowNonTsExtensions:true};
let revision=0;
const host:ts.LanguageServiceHost={getScriptFileNames:()=>Array.from(files.keys()),getScriptVersion:()=>String(revision),getScriptSnapshot:path=>{const text=files.get(path);return text===undefined?undefined:ts.ScriptSnapshot.fromString(text);},getCurrentDirectory:()=>"/",getCompilationSettings:()=>options,getDefaultLibFileName:()=>"/lib.es5.d.ts",fileExists:path=>files.has(path),readFile:path=>files.get(path),directoryExists:path=>Array.from(files.keys()).some(f=>f.startsWith(path.replace(/\/$/,"")+"/")),useCaseSensitiveFileNames:()=>true};
const service=ts.createLanguageService(host);
self.onmessage=(event:MessageEvent<{id:number;files:File[];path:string;offset:number;kind:"definition"|"references"|"hover"}>)=>{
 const {id,path,offset,kind}=event.data;
 try{
 files=new Map(event.data.files.map(f=>["/"+f.path.replace(/\\/g,"/"),f.content]));files.set("/lib.es5.d.ts",es5);files.set("/lib.dom.d.ts",dom);revision++;
 const name="/"+path;
 if(kind==="hover"){const info=service.getQuickInfoAtPosition(name,offset);self.postMessage({id,hover:info?ts.displayPartsToString(info.displayParts)+"\n"+ts.displayPartsToString(info.documentation):""});return;}
 const items=kind==="definition"?service.getDefinitionAtPosition(name,offset):service.getReferencesAtPosition(name,offset);
 const locations=(items??[]).filter(item=>!item.fileName.startsWith("/lib.")).map(item=>{const source=service.getProgram()?.getSourceFile(item.fileName);const pos=source?.getLineAndCharacterOfPosition(item.textSpan.start);return {path:item.fileName.replace(/^\//,""),line:(pos?.line??0)+1,column:(pos?.character??0)+1};});
 self.postMessage({id,locations});
 }catch(error){self.postMessage({id,error:String(error)});}
};
