// Isolated native API check: no user data or remote model calls.
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
const home = mkdtempSync(join(tmpdir(), 'simple-delete-test-'));
const cwd = join(home, 'project'); mkdirSync(cwd);
const settings = ['features.remote_models=false', 'features.plugins=false', 'features.apps=false', 'features.remote_control=false', 'analytics.enabled=false', 'feedback.enabled=false', 'check_for_update_on_startup=false', 'otel.exporter="none"', 'otel.trace_exporter="none"', 'otel.metrics_exporter="none"', 'model_provider="fixture"', 'model_providers.fixture.name="fixture"', 'model_providers.fixture.base_url="http://127.0.0.1:9/v1"', 'model_providers.fixture.wire_api="responses"', 'model_providers.fixture.requires_openai_auth=false'];
const child = spawn(resolve('kernels/packages/official-283-windows-candidate-1/codex-app-server.exe'), settings.flatMap(s => ['-c', s]), { env: { ...process.env, CODEX_HOME: home }, windowsHide: true, stdio: ['pipe','pipe','pipe'] });
child.stderr.resume();
const pending = new Map(); let id = 0;
createInterface({ input: child.stdout }).on('line', line => { try { const value=JSON.parse(line); const callback=pending.get(value.id); if(callback) { pending.delete(value.id); callback(value); } } catch {} });
const request = (method, params) => new Promise((resolve, reject) => {
  const key = ++id; const timer=setTimeout(()=>reject(new Error(`Timed out: ${method}`)),15000);
  pending.set(key, result=>{clearTimeout(timer); resolve(result);});
  child.stdin.write(JSON.stringify({id:key,method,params})+'\n');
});
try {
  const init = await request('initialize',{clientInfo:{name:'simple-fixture',version:'0.1.1'},capabilities:{experimentalApi:true}});
  if(init.error) throw new Error(JSON.stringify(init.error));
  child.stdin.write(JSON.stringify({method:'initialized'})+'\n');
  const started=await request('thread/start',{cwd,ephemeral:false,approvalPolicy:'never',sandbox:'read-only'});
  if(started.error) throw new Error(JSON.stringify(started.error));
  const threadId=started.result.thread.id;
  const injected = await request('thread/inject_items',{threadId,items:[{type:'message',role:'user',content:[{type:'input_text',text:'Temporary deletion fixture'}]}]});
  if(injected.error) throw new Error(JSON.stringify(injected.error));
  const deleted=await request('thread/delete',{threadId});
  console.log(JSON.stringify({operation:'delete_created_fixture',response:deleted}));
  if(deleted.error) throw new Error('Native delete failed');
  const reread=await request('thread/read',{threadId,includeTurns:false});
  console.log(JSON.stringify({operation:'read_deleted_fixture',response:reread}));
  if(!reread.error) throw new Error('Deleted thread is still readable');
  console.log(JSON.stringify({operation:'repeat_delete',response:await request('thread/delete',{threadId})}));
} finally { child.kill(); }
