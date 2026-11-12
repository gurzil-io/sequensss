#!/usr/bin/env bash
# Installs the Android toolchain into ~/android (Linux, no sudo): JDK 17, SDK, NDK, Gradle, cargo-ndk.
# Idempotent: safe to run again.
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"

ROOT="$HOME/android"
SDK="$ROOT/sdk"
NDK_VERSION="27.2.12479018"
PLATFORM="android-35"
BUILD_TOOLS="35.0.0"
GRADLE_VERSION="8.10.2"
CMDLINE_ZIP="commandlinetools-linux-11076708_latest.zip"

mkdir -p "$ROOT" "$SDK"
cd "$ROOT"

unzip_py() { python3 -m zipfile -e "$1" "$2"; }

# JDK 17 (Temurin)
if [ ! -x "$ROOT/jdk/bin/java" ]; then
  echo ">> JDK 17"
  curl -fsSL -o jdk.tar.gz "https://api.adoptium.net/v3/binary/latest/17/ga/linux/x64/jdk/hotspot/normal/eclipse"
  mkdir -p jdk && tar -xzf jdk.tar.gz -C jdk --strip-components=1 && rm jdk.tar.gz
fi
export JAVA_HOME="$ROOT/jdk"
export PATH="$JAVA_HOME/bin:$PATH"

# SDK command-line tools
if [ ! -x "$SDK/cmdline-tools/latest/bin/sdkmanager" ]; then
  echo ">> cmdline-tools"
  curl -fsSL -o cmdline.zip "https://dl.google.com/android/repository/$CMDLINE_ZIP"
  rm -rf cmdline-tmp && unzip_py cmdline.zip cmdline-tmp && rm cmdline.zip
  mkdir -p "$SDK/cmdline-tools" && rm -rf "$SDK/cmdline-tools/latest"
  mv cmdline-tmp/cmdline-tools "$SDK/cmdline-tools/latest" && rm -rf cmdline-tmp
  chmod +x "$SDK/cmdline-tools/latest/bin/"*
fi
SDKM="$SDK/cmdline-tools/latest/bin/sdkmanager"

echo ">> SDK packages"
yes | "$SDKM" --sdk_root="$SDK" --licenses > /dev/null || true
"$SDKM" --sdk_root="$SDK" "platform-tools" "platforms;$PLATFORM" "build-tools;$BUILD_TOOLS" "ndk;$NDK_VERSION"
# Zips extracted by Python lose the executable bit: restore it.
chmod -R +x "$SDK/platform-tools" "$SDK/build-tools" "$SDK/ndk/$NDK_VERSION/toolchains" 2>/dev/null || true

# Gradle (used to build the project)
if [ ! -x "$ROOT/gradle/bin/gradle" ]; then
  echo ">> Gradle $GRADLE_VERSION"
  curl -fsSL -o gradle.zip "https://services.gradle.org/distributions/gradle-$GRADLE_VERSION-bin.zip"
  rm -rf gradle-tmp && unzip_py gradle.zip gradle-tmp && rm gradle.zip
  rm -rf gradle && mv "gradle-tmp/gradle-$GRADLE_VERSION" gradle && rm -rf gradle-tmp
  chmod +x gradle/bin/gradle
fi

echo ">> Rust: Android targets + cargo-ndk"
rustup target add aarch64-linux-android x86_64-linux-android
command -v cargo-ndk > /dev/null || cargo install cargo-ndk --locked

cat > "$ROOT/env.sh" <<EOF
export JAVA_HOME="$ROOT/jdk"
export ANDROID_HOME="$SDK"
export ANDROID_NDK_HOME="$SDK/ndk/$NDK_VERSION"
export PATH="\$JAVA_HOME/bin:$SDK/platform-tools:$ROOT/gradle/bin:\$HOME/.cargo/bin:\$PATH"
EOF
echo "OK : source ~/android/env.sh"
