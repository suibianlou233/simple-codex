"""Read-only pinned-kernel catalog acceptance; uses an isolated CODEX_HOME."""
import json, os, queue, subprocess, tempfile, threading
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
EXE = ROOT / "kernels/packages/official-283-windows-candidate-1/codex-app-server.exe"
SKILL = Path.home() / ".codex/skills/write-serialized-novel"

def run(home):
    env = os.environ.copy(); env["CODEX_HOME"] = str(home)
    child = subprocess.Popen([str(EXE)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, encoding="utf-8", env=env, creationflags=subprocess.CREATE_NO_WINDOW)
    messages = queue.Queue()
    def pump():
        for line in child.stdout:
            try: messages.put(json.loads(line))
            except ValueError: pass
    threading.Thread(target=pump, daemon=True).start()
    seq = 0
    def request(method, params):
        nonlocal seq
        seq += 1; current = seq
        child.stdin.write(json.dumps({"id": current,"method":method,"params":params})+"\n"); child.stdin.flush()
        while True:
            msg = messages.get(timeout=30)
            if msg.get("id") == current:
                if "error" in msg: raise RuntimeError(msg["error"])
                return msg["result"]
    def listed():
        result=request("skills/list",{"cwds":[str(home)],"forceReload":True})
        return [s for group in result["data"] for s in group["skills"] if s["name"]=="write-serialized-novel"]
    try:
        request("initialize",{"clientInfo":{"name":"simple-skill-acceptance","version":"0.1.1"},"capabilities":{"experimentalApi":True}})
        child.stdin.write('{"method":"initialized"}\n');child.stdin.flush()
        request("skills/extraRoots/set",{"extraRoots":[str(SKILL)]})
        found=listed();assert len(found)==1,found
        request("skills/extraRoots/set",{"extraRoots":[]})
        assert not listed(),"disabled skill still listed"
        request("skills/extraRoots/set",{"extraRoots":[str(SKILL)]})
        assert len(listed())==1
        print("PASS: register, discover, disable, re-enable",flush=True)
    finally:
        child.terminate();child.wait(timeout=15)

if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="simple-skill-catalog-") as directory:
        home=Path(directory)
        run(home);run(home)
        print("PASS: isolated kernel restart")
