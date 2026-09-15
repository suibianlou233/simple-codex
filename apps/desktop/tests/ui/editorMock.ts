import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
export function installEditorMock() {
  mockWindows("main");
  const files:Record<string,{content:string;sha256:string}>={"src/main.ts":{content:"export const greeting = 'Hello Simple';\n",sha256:"initial"}};
  let maximized=false;
  const windowCalls:string[]=[];
  Object.assign(window,{__windowCalls:windowCalls});
  mockIPC((command,args)=>{
    if(command.startsWith("plugin:window|")){
      windowCalls.push(command);
      if(command==="plugin:window|is_maximized")return maximized;
      if(command==="plugin:window|toggle_maximize")maximized=!maximized;
      return;
    }
    const data=args as {path:string;content:string;expected:string;destination:string;query:string};
    if(command==="editor_list")return data.path===""?[{path:"src",name:"src",directory:true}]:Object.keys(files).map(path=>({path,name:path.split("/").pop(),directory:false}));
    if(command==="editor_read") {if(!files[data.path])throw new Error("路径不存在");return {path:data.path,...files[data.path]};}
    if(command==="editor_search")return Object.keys(files).filter(path=>path.includes(data.query));
    if(command==="editor_save"){if(files[data.path].sha256!==data.expected)throw new Error("文件发生变化");files[data.path]={content:data.content,sha256:crypto.randomUUID()};return {path:data.path,...files[data.path]};}
    if(command==="editor_create"){if(files[data.path])throw new Error("已存在");files[data.path]={content:"",sha256:crypto.randomUUID()};return;}
    if(command==="editor_rename"){files[data.destination]=files[data.path];delete files[data.path];return;}
    if(command==="test_external"){files["src/main.ts"]={content:"export const greeting = 'AI changed';\n",sha256:"external"};return;}
    return 1;
  });
}
