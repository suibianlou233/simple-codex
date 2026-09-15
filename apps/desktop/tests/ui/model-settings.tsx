import {useState} from "react";
import {createRoot} from "react-dom/client";
import {ModelSettings} from "../../src/settings/ModelSettings";
import "../../src/styles.css";
function Fixture(){
  const [saved,setSaved]=useState("");
  return <><ModelSettings disabled={false} onClose={()=>{}} activeProfile={{id:"existing",name:"DeepSeek",baseUrl:"https://api.deepseek.com",model:"deepseek-v4-flash",dialect:"deep_seek",timeoutMs:240000,maxOutputTokens:32768,contextWindowTokens:1048576,isDefault:true,hasCredential:true}}
    onSave={async input=>{setSaved(JSON.stringify(input));}}/><output data-testid="saved" hidden>{saved}</output></>;
}
createRoot(document.getElementById("root")!).render(<Fixture/>);
