#!/usr/bin/env bash
# Builds the Android APK (Linux/WSL or macOS). Requires ~/android/env.sh (see android/setup-toolchain.sh).
#   ./android/build-apk.sh            → debug APK (Rust in release mode), dist/sequensss.apk
#   ./android/build-apk.sh --rust     → Rust library only
#   ./android/build-apk.sh --install  → also install on the device (adb) and launch
#   ./android/build-apk.sh --release  → signed release APK (android/keystore.properties), dist/sequensss-release.apk
#   ./android/build-apk.sh --beta 0.2.0-beta.1
#                                     → signed beta APK, a separate app ("Sequensss Beta",
#                                       com.sequensss.app.beta), dist/sequensss-beta.apk
# Version: the workspace Cargo.toml (single source); versionCode = major*10000 + minor*100 + patch.
set -euo pipefail
cd "$(dirname "$0")/.."
source "$HOME/android/env.sh"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/sequensss-target}"

MODE="${1:-}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
IFS=. read -r MAJ MIN PAT <<< "${VERSION%%-*}"
VCODE=$((MAJ * 10000 + MIN * 100 + PAT))
if [ "$MODE" = "--beta" ]; then
  VERSION="${2:?usage: build-apk.sh --beta <version>, e.g. 0.2.0-beta.1}"
  # Embedded in the app (update check, User-Agent); read at compile time.
  export SEQUENSSS_VERSION="$VERSION"
  # The beta app has its own version codes: always increasing (commit count).
  VCODE="$(git rev-list --count HEAD)"
fi
GRADLE_PROPS=(-Psequensss.versionName="$VERSION" -Psequensss.versionCode="$VCODE")

# arm64: phones and tablets; x86_64: emulator / Chromebook.
ABIS="${ABIS:-arm64-v8a}"
TARGETS=()
for a in $ABIS; do TARGETS+=(-t "$a"); done

# Release and beta builds: rebuild the app crates so the version they embed is the current one.
if [ "$MODE" = "--release" ] || [ "$MODE" = "--beta" ]; then
  for a in $ABIS; do
    case $a in arm64-v8a) t=aarch64-linux-android ;; x86_64) t=x86_64-linux-android ;; *) continue ;; esac
    cargo clean -q --release --target "$t" -p sequensss -p seq-android
  done
fi
cargo ndk "${TARGETS[@]}" --platform 29 -o android/app/src/main/jniLibs build --release -p seq-android
[ "$MODE" = "--rust" ] && exit 0

# Gradle is slow on /mnt/c (WSL): build from a copy of the Gradle project.
WORK="$HOME/.cache/sequensss-gradle"
mkdir -p "$WORK"
cp -r android/. "$WORK/"
mkdir -p dist
if [ "$MODE" = "--release" ] || [ "$MODE" = "--beta" ]; then
  [ -f android/keystore.properties ] || { echo "android/keystore.properties is missing" >&2; exit 1; }
  if [ "$MODE" = "--beta" ]; then
    (cd "$WORK" && gradle --no-daemon -q assembleRelease "${GRADLE_PROPS[@]}" -Psequensss.beta=true)
    OUT="dist/sequensss-beta.apk"
  else
    (cd "$WORK" && gradle --no-daemon -q assembleRelease "${GRADLE_PROPS[@]}")
    OUT="dist/sequensss-release.apk"
  fi
  cp "$WORK/app/build/outputs/apk/release/app-release.apk" "$OUT"
  echo "OK : $OUT ($VERSION, code $VCODE)"
  exit 0
fi
(cd "$WORK" && gradle --no-daemon -q assembleDebug "${GRADLE_PROPS[@]}")
cp "$WORK/app/build/outputs/apk/debug/app-debug.apk" dist/sequensss.apk
echo "OK : dist/sequensss.apk"

if [ "$MODE" = "--install" ]; then
  adb install -r dist/sequensss.apk
  adb shell am start -n com.sequensss.app/.MainActivity
fi
