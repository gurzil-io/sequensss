# Audio sources

## `splash-mix.mp3`

The launch sound, mixed for Sequensss from three sounds from Pixabay, all under the
[Pixabay Content License](https://pixabay.com/service/license-summary/):

- "Snake hissing" by freesound_community (Pixabay #6092)
- "Bass slide" by freesound_community (Pixabay #39561)
- "Synth shot FX 20" by alien_i_trust (Pixabay #245448)

It becomes the launch splash sound, `crates/seq-app/assets/splash-hiss.wav`
(16-bit mono WAV at 44.1 kHz, the app has no MP3 decoder): decoded with `afconvert`
(macOS), then cut to its first 1.6 s (the rest is silence) with a 0.1 s fade out and
lowered by 1.7 dB (peak about -1.7 dBFS) with `ffmpeg`:

```bash
afconvert -f WAVE -d LEI16@44100 -c 1 splash-mix.mp3 /tmp/hiss.wav
ffmpeg -i /tmp/hiss.wav -af "atrim=0:1.6,afade=t=out:st=1.5:d=0.1,volume=-1.7dB" -c:a pcm_s16le \
  -map_metadata -1 -fflags +bitexact -flags:a +bitexact ../../crates/seq-app/assets/splash-hiss.wav
```

The app plays it 6 dB lower (`HISS_GAIN` in `crates/seq-app/src/lib.rs`).
