# Contributing to Sequensss

Thanks for helping. These are the project's rules: please read them before opening a pull request, and keep them in mind when changing code.

## The compass

Sequensss is a **lightweight MIDI sequencer for dawless setups**: one place to drive hardware synths, from an Android phone or tablet as well as a desktop. It must **not turn into a DAW**.

A feature gets in if it helps drive hardware, or makes sketching sequences faster, while staying light. Audio recording, audio tracks and other full-DAW features are out of scope. Bigger ideas (mixer, automation, plugins) only come in if they stay optional and lightweight. See the README's roadmap.

## Conventions

- **English everywhere in the code**: identifiers, comments, logs, error strings, scripts, docs and commit messages.
- **The UI is bilingual (French / English).** Every user-visible string goes through `tr!("fr", "en")` or `trf!("fr", "en", …)` from `seq-i18n`. `crates/seq-i18n/tests/translations.rs` checks that both halves have the same placeholders and punctuation.
- **Commit messages**: a short English subject, an optional body explaining why, and **no trailers** (no `Co-Authored-By`, no generated-by lines).
- **Match the surrounding code**: naming, idioms and comment density. Keep comments short and useful.
- **No secrets in git**: the Android signing key and `android/keystore.properties` stay out of the repository.

## Architecture rules

The workspace is split into crates with one-way dependencies (`seq-model` → `seq-edit` / `seq-engine` → `seq-io` / `seq-ui` → `seq-app`); see the README's architecture section.

- **The audio thread is real-time safe**: no locks, no allocation or deallocation, no I/O and no blocking syscalls in the audio callback. Communicate through the existing lock-free ring buffers (`rtrb`); objects to free go back through the `Garbage` path.
- **The UI never talks to the engine directly.** It edits the `Project`; `seq-app` compiles a `PlaybackGraph` and sends commands to the engine. `seq-ui` doesn't depend on `seq-engine`.
- **`seq-engine` knows nothing about cpal or egui**, which keeps it testable offline (the same renderer exports WAV files).
- **Edits go through `seq-edit`** so that undo and redo keep working: one gesture is one undo step.
- **Project files stay compatible.** New fields use `#[serde(default)]`, and old `.ron` files must keep loading (see `crates/seq-model/tests/`).
- **Platform code stays at the edges**: Android-specific code lives behind `#[cfg(target_os = "android")]` in `seq-io` / `seq-app` and in the Kotlin bridges under `android/`.

## Building and testing

You need Rust 1.85 or newer. On Linux, install `libasound2-dev libgtk-3-dev libxkbcommon-dev pkg-config` first.

```bash
cargo test --workspace            # all tests
cargo run --release -p sequensss  # desktop app
./build-macos.sh                  # macOS universal .app
./android/build-apk.sh            # Android debug APK (needs ~/android/env.sh, see android/setup-toolchain.sh)
```

Before pushing, check that:

- `cargo test --workspace` passes;
- `cargo build --release -p sequensss` builds without warnings;
- the Android library still builds (`./android/build-apk.sh --rust`) when you touched shared or Android code.

Add tests for new logic and for every bug you fix: the engine, `seq-edit` and `seq-io` are all testable without hardware. Timing tests must stay robust on busy CI machines: wait for an outcome rather than sleeping for a fixed time.

## How issues move

Issues are tracked on the [project board](https://github.com/orgs/gurzil-io/projects/1): **Backlog → Todo → In progress → To test → Done**.

1. Work on an issue moves it to **In progress**. Commits reference it with `Refs #N`. Don't use `Closes #N` or `Fixes #N`, which would close the issue before anyone tested it.
2. When the change is ready, the issue moves to **To test** and the **beta** is updated (see below). A comment on the issue says what to check.
3. The tester tries it in the beta. If it works, they close the issue and it moves to **Done**. If not, they comment and it goes back to **In progress**.

## Versions

[Semantic versioning](https://semver.org): `MAJOR.MINOR.PATCH`. While in `0.x`, a minor bump (`0.2.0`) brings features or breaking changes, and a patch bump (`0.2.1`) only fixes bugs. Betas are previews of the next stable version: `0.2.0-beta.1`, `0.2.0-beta.2`, then `0.2.0`.

The version lives in one place: the workspace `Cargo.toml`. The Android `versionCode` is derived from it (`major * 10000 + minor * 100 + patch`).

## Releases

### Beta

There is a single beta, always at the same link: [releases/tag/beta](https://github.com/gurzil-io/sequensss/releases/tag/beta). On Android it is a separate app, **Sequensss Beta** (`com.sequensss.app.beta`), installed next to the stable one.

1. `./android/build-apk.sh --beta 0.X.Y-beta.N` builds `dist/sequensss-beta.apk`.
2. Move the tag: `git tag -f beta && git push -f origin beta`.
3. Update the pre-release: `gh release upload beta dist/sequensss-beta.apk --clobber` and `gh release edit beta --notes-file <notes>`. The notes list every issue in **To test** and what to check.
4. Rebuild the desktop files: `gh workflow run release.yml -f version=0.X.Y-beta.N`.

The in-app update check never offers betas.

### Stable

1. Set the version in the workspace `Cargo.toml` and commit `Version X.Y.Z`.
2. `./android/build-apk.sh --release` builds the signed APK.
3. `gh release create vX.Y.Z --notes-file <notes> dist/sequensss-release.apk`, after renaming the APK to `sequensss-X.Y.Z.apk`. Creating the tag starts CI, which runs the tests and attaches the Windows, macOS and Linux builds.
