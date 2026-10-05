import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
export function installEditorMock() {
  mockWindows("main");
  const files:Record<string,{content:string;sha256:string}>={"src/main.ts":{content:"export const greeting = 'Hello Simple';\n",sha256:"initial"}};
  let maximized=false;
  const windowCalls:string[]=[];
  const requests:unknown[]=[],saves:unknown[]=[];
  Object.assign(window,{__windowCalls:windowCalls,__editorRequests:requests,__editorSaves:saves});
  mockIPC((command,args)=>{
    if(command.startsWith("plugin:window|")){
      windowCalls.push(command);
      if(command==="plugin:window|is_maximized")return maximized;
      if(command==="plugin:window|toggle_maximize")maximized=!maximized;
      return;
    }
    if(command==="editor_assist"){const input=(args as {input:{mode:string}}).input;requests.push(input);return input.mode==="review"?JSON.stringify([{path:"src/main.ts",line:1,severity:"medium",title:"缺少校验",detail:"模拟审查发现"}]):"1. 检查 src/main.ts\n2. 修改欢迎语\n3. 执行测试";}
    if(command==="editor_language_files")return {files:Object.entries(files).map(([path,value])=>({path,...value})),limited:false};
    if(command==="editor_grep")return {hits:[{path:"src/main.ts",line:1,column:14,text:files["src/main.ts"].content}],limited:false,scanned:1};
    if(command==="test_create_external"){files["src/added.ts"]={content:"export const n=1;",sha256:"new"};return;}
    if(command==="test_delete_external"){delete files["src/main.ts"];return;}
    if(command==="editor_suggest") {
      const input=(args as {input:{content:string;start:number;end:number;instruction:string}}).input;
      requests.push(input);
      return new Promise(resolve=>setTimeout(()=>resolve(input.content.slice(input.start,input.end).replace("Hello Simple","Hello Cursor")),200));
    }
    if(command==="editor_cancel_suggestion")return;
    const data=args as {path:string;content:string;expected:string;destination:string;query:string};
    if(command==="editor_list")return data.path===""?[{path:"src",name:"src",directory:true}]:Object.keys(files).map(path=>({path,name:path.split("/").pop(),directory:false}));
    if(command==="editor_read") {if(!files[data.path])throw new Error("路径不存在");return {path:data.path,...files[data.path]};}
    if(command==="editor_search")return Object.keys(files).filter(path=>path.includes(data.query));
    if(command==="editor_save"){saves.push(data);if(files[data.path].sha256!==data.expected)throw new Error("文件发生变化");files[data.path]={content:data.content,sha256:crypto.randomUUID()};return {path:data.path,...files[data.path]};}
    if(command==="editor_create"){if(files[data.path])throw new Error("已存在");files[data.path]={content:"",sha256:crypto.randomUUID()};return;}
    if(command==="editor_rename"){files[data.destination]=files[data.path];delete files[data.path];return;}
    if(command==="test_crlf"){files["src/main.ts"]={content:"export const greeting = 'Hello Simple';\r\n// second line\r\n",sha256:"crlf"};return;}
    if(command==="test_external"){files["src/main.ts"]={content:"export const greeting = 'AI changed';\n",sha256:"external"};return;}
    return 1;
  });
}
