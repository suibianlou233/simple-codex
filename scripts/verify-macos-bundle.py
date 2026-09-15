"""Fail packaging if bundling/signing changes or drops a frozen kernel component."""
import hashlib
import json
from pathlib import Path
import sys


def verify(app):
    root = app / "Contents/Resources/kernels/official-283-macos-candidate-1"
    manifest = json.loads((root / "kernel.json").read_text(encoding="utf-8"))
    for name, expected in manifest["files"].items():
        if name != Path(name).name or name in (".", ".."):
            raise ValueError("Non-flat kernel package")
        path = root / name
        if path.is_symlink() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError("Bundled kernel hash mismatch: " + name)
    print("Bundled kernel hashes verified: " + manifest["target"])


if __name__ == "__main__":
    verify(Path(sys.argv[1]).resolve())
