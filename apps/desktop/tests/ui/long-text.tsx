import { useState } from "react";
import { createRoot } from "react-dom/client";
import { Composer } from "../../src/components/Composer";
import { pasteIntoDraft, type TextDraft } from "../../src/app/textDraft";
import "../../src/styles.css";
import "../../src/design/workbench.css";
function Fixture() {
  const [draft, setDraft] = useState<TextDraft>({content:"",pastes:[]});
  const [sent, setSent] = useState("");
  return <><Composer projectId="p" projectName="我的项目" activeTurnId={null} disabled={false} content={draft.content}
    pastes={draft.pastes} onTextPaste={(text,start,end)=>setDraft(current=>pasteIntoDraft(current,text,start,end,crypto.randomUUID()))}
    onRemovePaste={id=>setDraft(current=>({...current,pastes:current.pastes.filter(block=>block.id!==id)}))}
    onEditPaste={(id,text)=>setDraft(current=>({...current,pastes:current.pastes.map(block=>block.id===id?{...block,text}:block)}))}
    attachments={[]} onContentChange={content=>setDraft(current=>({...current,content}))} onAttach={async () => {}}
    onRemoveAttachment={() => {}} onSend={async text => { setSent(text); }}
    onCancel={async () => false} permissionLevel="approval" permissionScopeLabel="当前任务"
    workspaceSandboxReady={false} permissionDisabled={false} onPermissionChange={async () => true} />
    <output data-testid="sent" hidden>{sent}</output></>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
