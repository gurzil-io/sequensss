# Audio sources

## `freesound_community-snake-hissing-6092.mp3`

"Snake hissing" by freesound_community, from Pixabay, under the
[Pixabay Content License](https://pixabay.com/service/license-summary/).

It becomes the launch splash hiss, `crates/seq-app/assets/splash-hiss.wav`
(16-bit mono WAV at 44.1 kHz, the app has no MP3 decoder): decoded with `afconvert`
(macOS), then the near-silent first 50 ms and the noise tail after 1.85 s cut off and
the level raised by 13.5 dB (peak about -1.7 dBFS) with `ffmpeg`:

```bash
afconvert -f WAVE -d LEI16@44100 -c 1 freesound_community-snake-hissing-6092.mp3 /tmp/hiss.wav
ffmpeg -i /tmp/hiss.wav -af "atrim=0.05:1.85,volume=13.5dB" -c:a pcm_s16le \
  -map_metadata -1 -fflags +bitexact -flags:a +bitexact ../../crates/seq-app/assets/splash-hiss.wav
```

The app plays it 6 dB lower (`HISS_GAIN` in `crates/seq-app/src/lib.rs`).
