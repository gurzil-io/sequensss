#!/usr/bin/env bash
# Builds the Android APK (Linux/WSL or macOS). Requires ~/android/env.sh (see android/setup-toolchain.sh).
#   ./android/build-apk.sh            → debug APK (Rust in release mode), dist/sequensss.apk
#   ./android/build-apk.sh --rust     → Rust library only
#   ./android/build-apk.sh --install  → also install on the device (adb) and launch
#   ./android/build-apk.sh --release  → signed release APK (android/keystore.properties), dist/sequensss-release.apk
#   ./android/build-apk.sh --test 0.1.5-test.1
#                                     → signed test APK, a separate app ("Sequensss Test",
#                                       com.sequensss.app.test), dist/sequensss-test-0.1.5-test.1.apk
set -euo pipefail
cd "$(dirname "$0")/.."
source "$HOME/android/env.sh"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/sequensss-target}"

MODE="${1:-}"
if [ "$MODE" = "--test" ]; then
  TEST_VERSION="${2:?usage: build-apk.sh --test <version>, e.g. 0.1.5-test.1}"
  # Embedded in the app (update check, User-Agent); read at compile time.
  export SEQUENSSS_VERSION="$TEST_VERSION"
fi

# arm64: phones and tablets; x86_64: emulator / Chromebook.
ABIS="${ABIS:-arm64-v8a}"
TARGETS=()
for a in $ABIS; do TARGETS+=(-t "$a"); done

# Release and test builds: rebuild the app crates so the version they embed is the current one.
if [ "$MODE" = "--release" ] || [ "$MODE" = "--test" ]; then
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
if [ "$MODE" = "--release" ] || [ "$MODE" = "--test" ]; then
  [ -f android/keystore.properties ] || { echo "android/keystore.properties is missing" >&2; exit 1; }
  if [ "$MODE" = "--test" ]; then
    # Test app: its own version code, always increasing (commit count).
    VCODE="$(git rev-list --count HEAD)"
    (cd "$WORK" && gradle --no-daemon -q assembleRelease -Psequensss.test=true \
      -Psequensss.versionName="$TEST_VERSION" -Psequensss.versionCode="$VCODE")
    OUT="dist/sequensss-test-$TEST_VERSION.apk"
  else
    (cd "$WORK" && gradle --no-daemon -q assembleRelease)
    OUT="dist/sequensss-release.apk"
  fi
  cp "$WORK/app/build/outputs/apk/release/app-release.apk" "$OUT"
  echo "OK : $OUT"
  exit 0
fi
(cd "$WORK" && gradle --no-daemon -q assembleDebug)
cp "$WORK/app/build/outputs/apk/debug/app-debug.apk" dist/sequensss.apk
echo "OK : dist/sequensss.apk"

if [ "$MODE" = "--install" ]; then
  adb install -r dist/sequensss.apk
  adb shell am start -n com.sequensss.app/.MainActivity
fi
