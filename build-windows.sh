#!/usr/bin/env bash
# Cross-compiles the Windows build from Linux/WSL (avoids Smart App Control on the Windows side).
# Requires: sudo apt install -y mingw-w64 ; rustup target add x86_64-pc-windows-gnu
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
PROFILE="${1:-release}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/sequensss-target}"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
if [ "$PROFILE" = "release" ]; then
  cargo build --release --target x86_64-pc-windows-gnu -p sequensss
else
  cargo build --target x86_64-pc-windows-gnu -p sequensss
fi
mkdir -p dist
cp "$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/$PROFILE/sequensss.exe" dist/
echo "OK : dist/sequensss.exe"
