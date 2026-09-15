import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("macos_kernel", Path(__file__).with_name("build-macos-kernel.py"))
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class MacKernelArtifacts(unittest.TestCase):
    def test_cached_pairs_are_verified_for_both_targets(self):
        for target in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                (root / "codex-rs").mkdir()
                (root / "codex-rs/Cargo.lock").write_text('[[package]]\nname = "v8"\nversion = "150.4.0"\n', encoding="utf-8")
                archive = f"librusty_v8_ptrcomp_sandbox_release_{target}.a.gz"
                binding = f"src_binding_ptrcomp_sandbox_release_{target}.rs"
                files = {name: hashlib.sha256(name.encode()).hexdigest() for name in (archive, binding)}
                pins = root / "pins.json"
                pins.write_text(json.dumps({"upstream_revision":build.REVISION,"version":"150.4.0","targets":{target:files}}), encoding="utf-8")
                for name in files:
                    (root / name).write_bytes(name.encode())
                with patch.object(build,"V8_PINS",pins), patch.object(build.subprocess,"run") as download:
                    env, _ = build.prepare_v8(root,target,root)
                    self.assertEqual(Path(env["RUSTY_V8_ARCHIVE"]).name,archive)
                    self.assertEqual(Path(env["RUSTY_V8_SRC_BINDING_PATH"]).name,binding)
                    download.assert_not_called()
                    (root / binding).write_bytes(b"tampered")
                    with self.assertRaisesRegex(ValueError,"checksum mismatch"):
                        build.prepare_v8(root,target,root)
                    (root / "codex-rs/Cargo.lock").write_text('name = "v8"\nversion = "1.0.0"\n',encoding="utf-8")
                    with self.assertRaisesRegex(ValueError,"Cargo.lock"):
                        build.prepare_v8(root,target,root)


if __name__ == "__main__":
    unittest.main()
