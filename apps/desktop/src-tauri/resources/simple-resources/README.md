# Simple Windows sandbox resources

This directory receives the two Simple-owned Windows sandbox helper binaries.
Run `scripts/sync-simple-sandbox-runtime.ps1` before a release build. During
rapid development the desktop process discovers the debug helpers directly
under `target/debug`, so no installer build is required.
