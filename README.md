<p align="center"><img src="assets/icon/sequensss-256.png" width="160" alt="Sequensss logo"></p>

<h1 align="center">Sequensss</h1>

<p align="center"><b>A lightweight MIDI sequencer for dawless setups: one place to drive all your hardware synths, from an Android phone or tablet.</b><br>Also runs on Windows, macOS and Linux.</p>

<p align="center">
  <a href="https://github.com/gurzil-io/sequensss/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/gurzil-io/sequensss?style=for-the-badge&label=release&color=f28c28&labelColor=1e1a17"></a>
  <a href="https://github.com/gurzil-io/sequensss/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/gurzil-io/sequensss/total?style=for-the-badge&label=downloads&color=46b03c&labelColor=1e1a17"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-efe6d0?style=for-the-badge&labelColor=1e1a17"></a>
</p>
<p align="center">
  <a href="#download"><img alt="Platforms: Windows, macOS, Linux, Android" src="https://img.shields.io/badge/platforms-Windows%20%C2%B7%20macOS%20%C2%B7%20Linux%20%C2%B7%20Android-4e2c16?style=flat-square&labelColor=1e1a17"></a>
  <a href="https://www.rust-lang.org"><img alt="Rust 1.85+" src="https://img.shields.io/badge/Rust-1.85%2B-f28c28?style=flat-square&logo=rust&logoColor=white&labelColor=1e1a17"></a>
  <a href="https://github.com/emilk/egui"><img alt="Built with egui" src="https://img.shields.io/badge/built%20with-egui-46b03c?style=flat-square&labelColor=1e1a17"></a>
</p>

## Why

I have a bunch of analog synths and wanted one central place to sequence them all. Doing it from a computer always felt like a chore: a DAW, drivers, routing, too much software between me and the knobs. Sequensss moves that brain to an Android phone or tablet. Plug in a USB MIDI interface, and each synth gets its own tracks, patterns and MIDI channel.

The goal is to make life easier for dawless setups **without becoming a DAW**:

- **One hub for all your synths.** Up to 8 MIDI outputs and any MIDI channel per track, with tight timing. MIDI in lets you play or record from a keyboard.
- **In your pocket.** USB MIDI and low-latency audio on Android, with touch-friendly layouts for phones and tablets. The same app runs on desktop.
- **Write offline, play later.** Work on your sequences anywhere (on the train, on the couch) with nothing plugged in, then connect the rig and press play. Built-in drums and a simple synth let you hear ideas without any hardware.
- **Stays lightweight.** Patterns, a step sequencer, a piano roll, a playlist and MIDI. No audio tracks, no plugin chain, no mixer to manage. One small app that starts instantly.

> **Vibe-coded, and open to everyone, forever.** Sequensss was built by vibe coding: fast, by ear, for the fun of making music software. It is MIT-licensed and will stay free and open. Fork it, hack it, bend it into your own instrument: no permission needed. See [Make it yours](#make-it-yours).

---

## Features

**Driving your synths (MIDI)**
- **MIDI out:** up to 8 outputs, a MIDI channel and an output per track. Events are scheduled against the audio output latency, so hardware and built-in sounds stay in sync.
- **MIDI clock out:** per output, 24 PPQN clock with Start / Stop / Continue and Song Position Pointer, so drum machines, arpeggiators and grooveboxes follow the sequencer.
- **MIDI in:** play the selected track live from a keyboard, or record into a pattern with optional quantize. Notes land where you played them.
- **USB MIDI on Android** (interfaces and controllers, hot-plug), and any MIDI port on desktop.
- **Metronome.**

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

**Audio engine**
- **Real-time safe:** no locks and no allocations in the audio callback, with sample-accurate event timing whatever the buffer size.

**Files**
- Projects are saved as human-readable `.ron` files, written atomically.
- Export to **WAV** (16-bit) and **Standard MIDI File** (one track per channel).

**Android**
- Low-latency audio through Oboe/AAudio, and **USB MIDI** through the Android MidiManager.
- Touch gestures, a full-screen mode, and layouts for tablets and phones.
- A built-in project browser, WAV import, and sharing of exports through the Android share sheet.

**English and French UI**, picked automatically from the system language.

---

## Download

Grab the latest build from the [**Releases**](https://github.com/gurzil-io/sequensss/releases) page.

| Platform | File | Notes |
|---|---|---|
| Windows 10/11 (x64) | `Sequensss-windows-x64.zip` | Unzip and run `sequensss.exe`. SmartScreen may warn that the app is unsigned: click *More info → Run anyway*. |
| macOS 11+ (Apple Silicon & Intel) | `Sequensss-macos.zip` | Unzip and move `Sequensss.app` to Applications. The app is not notarized, so macOS blocks the first launch: see [below](#macos-app-is-blocked). |
| Linux (x64) | `Sequensss-linux-x64.tar.gz` | Extract and run `./sequensss`. Requires ALSA and GTK 3, which most desktop distributions already have. |
| Android 10+ (arm64) | `sequensss-<version>.apk` | Allow installs from unknown sources, then open the APK. |

### Updates

At startup, at most once a day, the app asks GitHub whether a newer release exists and shows a small notice if it does. Nothing is sent besides that request. You can turn the check off with **Check for updates** in the file menu (≡ menu on phones and tablets).

### macOS: app is blocked

macOS says *"Sequensss" Not Opened* because the app is not notarized by Apple. To open it anyway, use either method:

- Try to open the app once and click **Done**. Go to **System Settings → Privacy & Security**, scroll down to the message about Sequensss and click **Open Anyway**. Then launch the app again and confirm.
- Or remove the quarantine flag in Terminal:
  ```bash
  xattr -dr com.apple.quarantine /Applications/Sequensss.app
  ```

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
- **Channels and patterns are copy-on-write** (`CowArc`): undo snapshots share everything a gesture did not modify, comparing two snapshots stops at the shared parts, and only the modified patterns are recompiled.
- The **engine knows nothing about cpal or egui**: it exposes `Engine::process(&mut [f32], …)`, which is why it can be tested offline and reused for WAV export.
- **Note-offs are derived from note lengths** inside the engine, so swapping the graph mid-note can never leave a note stuck.

---

## Roadmap

The compass: **make dawless setups easier, stay light.** A feature gets in if it helps drive hardware, or makes sketching sequences faster, without turning Sequensss into a DAW.

**Next, for the hardware rig**
- **Swap a track between a built-in sound and MIDI out** while keeping its notes: sketch on the built-in synth, then send it to the real one ([#9](https://github.com/gurzil-io/sequensss/issues/9)).

**Ideas, only if they stay optional and lightweight**
- Automation and tempo changes ([#6](https://github.com/gurzil-io/sequensss/issues/6)).
- A small mixer with a few effects for the built-in sounds ([#5](https://github.com/gurzil-io/sequensss/issues/5)).
- VST3 / CLAP plugins on desktop ([#7](https://github.com/gurzil-io/sequensss/issues/7)).

**Not planned:** audio recording, audio tracks and other full-DAW features. Your synths stay the sound source.

---

## Make it yours

This project belongs to everyone who uses it, so don't hesitate to change it.

- **Fork it** and turn it into your own instrument. The MIT license lets you use, modify and share it, commercially or not, forever.
- **Send a pull request**, big or small: a fix, a new sound, a translation, a UI tweak. The tests (`cargo test --workspace`) and the [architecture notes](#architecture) help you find your way around.
- **Open an issue** for a bug or an idea. The [issue list](https://github.com/gurzil-io/sequensss/issues) is a good place to start; the `nice to have` label marks the bigger projects.

---

## License

[MIT](LICENSE) © 2026 Mouad EL Moudni.

The bundled fonts (Barlow, Barlow Condensed, JetBrains Mono, Michroma, VT323) are distributed under the SIL Open Font License. Their licenses are in [`crates/seq-ui/assets/fonts`](crates/seq-ui/assets/fonts).
