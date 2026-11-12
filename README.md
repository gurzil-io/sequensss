<p align="center"><img src="assets/icon/sequensss-256.png" width="160" alt="Sequensss logo"></p>

<h1 align="center">Sequensss</h1>

**A lightweight MIDI sequencer and drum machine written in Rust.** It runs on Windows, macOS, Linux and Android.

Sequensss takes the workflow of pattern-based DAWs (a channel rack, step sequencer, piano roll and playlist) and keeps only the parts you need to sketch a beat quickly. It is a single native binary with no installer or runtime, it plays sound as soon as it opens (the drum kits are synthesized, so no sample packs are needed), and it can drive your hardware synths over MIDI.

[![CI](https://github.com/gurzil-io/sequensss/actions/workflows/ci.yml/badge.svg)](https://github.com/gurzil-io/sequensss/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## Features

**Making beats**
- **Channel rack and step sequencer.** Click or drag to paint steps. Each channel has volume, pan, mute, solo, a level meter and a choke group (for closed/open hi-hats). Pattern length is variable, and swing is global.
- **4 built-in kits** (Basic, 808, Hard, Synth starter), each with a demo beat. Every sound is synthesized at startup.
- **Sampler.** Load your own WAV files (8 to 32-bit, integer or float, any sample rate).
- **Synth.** Two polyBLEP oscillators (saw, square, sine, triangle), a resonant low-pass filter with an envelope, ADSR and 8 voices.
- **Arpeggiator per channel.** 8 modes, rates from 1/1 to 1/32 with triplets, 1 to 4 octaves, gate, chords, hold and presets. You can also "bake" an arpeggio into real notes.

**Editing**
- **Piano roll.** Multi-select, copy, paste, duplicate, transpose, snap from 1/4 to 1/32 or triplets, a velocity lane and zoom. The step sequencer and the piano roll edit the same notes.
- **Playlist.** Arrange patterns into a song with clips that repeat or truncate, a loop region and multi-select.
- **Undo/redo.** One gesture is one undo step, with up to 200 steps.
- **Paintbrush tool** in both the piano roll and the playlist.

**MIDI and audio**
- **MIDI in:** play the selected channel live from a keyboard, or record into a pattern with optional quantize.
- **MIDI out:** sequence external synths. Events are timestamped against the audio output latency.
- **Real-time-safe audio engine:** no locks and no allocations in the audio callback, with sample-accurate event timing whatever the buffer size.
- **Metronome.**

**Files**
- Projects are saved as human-readable `.ron` files, written atomically.
- Export to **WAV** (16-bit) and **Standard MIDI File** (one track per channel).

**Android**
- Low-latency audio through Oboe/AAudio, and **USB MIDI** controllers through the Android MidiManager.
- Touch gestures, a full-screen mode, and layouts for tablets and phones.
- A built-in project browser, WAV import, and sharing of exports through the Android share sheet.

**English and French UI**, picked automatically from the system language.

---

## Download

Grab the latest build from the [**Releases**](https://github.com/gurzil-io/sequensss/releases) page.

| Platform | File | Notes |
|---|---|---|
| Windows 10/11 (x64) | `Sequensss-windows-x64.zip` | Unzip and run `sequensss.exe`. SmartScreen may warn that the app is unsigned: click *More info → Run anyway*. |
| macOS 11+ (Apple Silicon & Intel) | `Sequensss-macos.zip` | Unzip and move `Sequensss.app` to Applications. The first time, **right-click → Open**, because the app is not notarized. |
| Linux (x64) | `Sequensss-linux-x64.tar.gz` | Extract and run `./sequensss`. Requires ALSA and GTK 3, which most desktop distributions already have. |
| Android 10+ (arm64) | `sequensss-<version>.apk` | Allow installs from unknown sources, then open the APK. |

---

## Quick start

1. Launch the app. A demo beat is already loaded, so press **Space** to play it.
2. Click the step buttons in the **channel rack** to change the drum pattern.
3. Double-click a channel name to open the **piano roll** and write melodies.
4. Create more patterns with **+ New pattern**, place them in the **playlist**, then switch the transport from **PAT** to **SONG**.
5. Open **Settings…** to choose your audio device and your MIDI input and output ports.
6. Use **Export WAV…** or **Export MIDI…** in the file menu.

| Shortcut | Action |
|---|---|
| `Space` | Play / pause |
| `Ctrl/Cmd + Z`, `Ctrl/Cmd + Y` | Undo / redo |
| `Ctrl/Cmd + S` | Save |
| `Ctrl/Cmd + A / C / V / D` | Select all / copy / paste / duplicate (piano roll and playlist) |
| `Delete` | Delete selection |
| Arrow keys | Transpose or shift the selected notes |
| `Ctrl/Cmd + mouse wheel` | Zoom |

---

## Building from source

You need **Rust 1.85 or newer** (the project uses edition 2024).

```bash
git clone https://github.com/gurzil-io/sequensss.git
cd sequensss
cargo run --release -p sequensss
```

### Linux dependencies

```bash
# Debian / Ubuntu
sudo apt install libasound2-dev libgtk-3-dev libxkbcommon-dev pkg-config
```

### Platform packages

| Target | Command | Output |
|---|---|---|
| macOS universal `.app` | `./build-macos.sh` | `dist/Sequensss.app`, `dist/Sequensss-macos.zip` |
| Windows (cross-compiled from Linux/WSL) | `./build-windows.sh` | `dist/sequensss.exe` |
| Android APK | `./android/build-apk.sh [--release] [--install]` | `dist/sequensss.apk` |

The Android build needs the SDK, NDK r27, JDK 17+, Gradle 8.10 and `cargo-ndk`. On Linux, `android/setup-toolchain.sh` installs all of them into `~/android`. For a signed release build, create `android/keystore.properties` (it is git-ignored):

```properties
storeFile=/absolute/path/to/your.jks
storePassword=...
keyAlias=...
keyPassword=...
```

### Tests

```bash
cargo test --workspace
```

The engine tests check sample-exact event timing at several buffer sizes, the absence of clock drift, loop boundaries, graph swaps during playback, choke groups and offline rendering. They do not need an audio device.

---

## Architecture

The workspace is split into small crates with one-way dependencies:

```
crates/
├── seq-model     Project data model and kits. Pure: serde, no I/O, no threads
├── seq-edit      Editing operations and snapshot-based undo/redo
├── seq-engine    Audio engine: transport, scheduler, instruments, mixer. Real-time safe
├── seq-io        Audio and MIDI back ends (cpal/midir on desktop, Oboe/JNI on Android), WAV, .ron, export
├── seq-ui        egui interface: transport, channel rack, piano roll, playlist, settings
├── seq-i18n      English and French strings
├── seq-app       The `sequensss` library and desktop binary
└── seq-android   Android entry point (cdylib)
```

- The **UI never talks to the engine directly.** It edits the `Project`. Each frame, the app diffs the project, compiles a `PlaybackGraph` and sends it to the audio thread through lock-free ring buffers (`rtrb`).
- The **engine knows nothing about cpal or egui**: it exposes `Engine::process(&mut [f32], …)`, which is why it can be tested offline and reused for WAV export.
- **Note-offs are derived from note lengths** inside the engine, so swapping the graph mid-note can never leave a note stuck.

---

## Roadmap

These features are out of scope for v0.1 and could come later:
- VST3 / CLAP plugin hosting
- A mixer with effects
- Automation and tempo changes
- MIDI clock sync
- Audio recording

Issues and pull requests are welcome.

---

## License

[MIT](LICENSE) © 2026 Mouad EL Moudni.

The bundled fonts (Barlow, Barlow Condensed, JetBrains Mono, Michroma, VT323) are distributed under the SIL Open Font License. Their licenses are in [`crates/seq-ui/assets/fonts`](crates/seq-ui/assets/fonts).
