"""Build a native, pinned, unmodified Mac kernel. Never imports Windows binaries."""
import hashlib
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
REVISION = "28327355b861ab6cc76b01c7248663eb1be440cf"
BINS = ("codex-app-server", "codex-code-mode-host", "apply_patch")


def run(*args, cwd=ROOT):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def main():
    if platform.system() != "Darwin" or platform.machine() not in ("arm64", "x86_64"):
        raise SystemExit("Run on a native arm64 or x86_64 Mac, not Windows or Rosetta.")
    translated = subprocess.run(["sysctl", "-in", "sysctl.proc_translated"], capture_output=True, text=True)
    if translated.stdout.strip() == "1":
        raise SystemExit("Use a native terminal, not Rosetta.")
    arch = "aarch64" if platform.machine() == "arm64" else "x86_64"
    target = arch + "-apple-darwin"
    source = ROOT / "upstream-codex"
    if not source.exists():
        subprocess.run(["git", "clone", "--no-checkout", "https://github.com/openai/codex.git", str(source)], check=True)
        subprocess.run(["git", "checkout", "--detach", REVISION], cwd=source, check=True)
    if run("git", "rev-parse", "HEAD", cwd=source) != REVISION or run("git", "status", "--porcelain", cwd=source):
        raise SystemExit("Upstream must be clean and exactly pinned; refusing to reset your checkout.")
    output = ROOT / "kernels/packages/official-283-macos-candidate-1"
    if output.exists():
        raise SystemExit("Package already exists. Move it aside explicitly before rebuilding.")
    workspace = source / "codex-rs"
    env = {**os.environ, "MACOSX_DEPLOYMENT_TARGET": "14.0", "CARGO_TARGET_DIR": str(source / "target-simple-macos")}
    command = ["cargo", "build", "--release", "--locked", "--target", target]
    for package in ("codex-app-server", "codex-code-mode-host", "codex-apply-patch"):
        command += ["-p", package]
    for binary in BINS:
        command += ["--bin", binary]
    subprocess.run(command, cwd=workspace, env=env, check=True)
    output.mkdir(parents=True)
    for binary in BINS:
        built = Path(env["CARGO_TARGET_DIR"]) / target / "release" / binary
        if run("lipo", "-archs", str(built)) != platform.machine():
            raise SystemExit("Unexpected binary architecture: " + binary)
        shutil.copy2(built, output / binary)
    for notice in ("LICENSE", "NOTICE"):
        shutil.copy2(source / notice, output / notice)
    (output / "changes.patch").write_bytes(b"")
    (output / "MODIFICATIONS.md").write_text(
        "# Mac candidate\n\nUnmodified OpenAI Codex at " + REVISION +
        ". No Windows V8 patch applied. Simple's adaptation is outside this package.\n"
        "Built with the pinned upstream Rust toolchain for " + target +
        ". Native execution, sandbox and recovery require acceptance before release.\n", encoding="utf-8")
    (output / "PATCHES.json").write_text(json.dumps({
        "schema_version": 1, "upstream_revision": REVISION, "files": [],
        "reason": "No source patches on macOS; retain upstream V8 sandbox features.",
        "patch_sha256": hashlib.sha256(b"").hexdigest(), "test_results": "build-only; runtime acceptance pending"
    }, indent=2) + "\n", encoding="utf-8")
    # Reuse only the registered protocol contract, never another platform's hashes.
    manifest = json.loads((ROOT / "kernels/packages/official-283-windows-candidate-1/kernel.json").read_text(encoding="utf-8"))
    manifest.update(id="official-283-macos-candidate-1", target=target,
                    files={p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(output.iterdir())})
    (output / "kernel.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print("Built and recorded " + target + " kernel: " + str(output))


if __name__ == "__main__":
    main()
