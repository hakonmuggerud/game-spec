# One-shot renderer — historical

The committed `undercroft/assets/audio/*.wav` and `manifest.ron` are the source now; this tool is
kept as the record of how they were made from the prototype and is not part of any build step.

`render.mjs` baked every one-shot in `reference/prototype/src/audio.js`'s `SOUNDS` table (plus the appended
menu cues and the creature roster) into `undercroft/assets/audio/*.wav` and writes
`undercroft/assets/audio/manifest.ron` beside them.

```sh
cd reference/tools/export/audio
npm install          # node-web-audio-api (Rust-backed Web Audio, real OfflineAudioContext)
node render.mjs
```

Why a renderer at all: the prototype had no sound assets — each one-shot was a fresh Web Audio
graph. The Bevy port keeps the *continuous* layers procedural (`crates/undercroft/src/audio/synth.rs`)
but plays the one-shots as clips, so they are rendered once, offline, from the very same
schedulers. `render.mjs` copies `gain/osc/filt/noiseSrc/out/env/note/burst/chime/chitter/bell` and
the whole `SOUNDS` table out of `audio.js` verbatim; only three things differ:

- `Math.random` is replaced by a seeded PRNG, so the noise buffer and the `rnd()` pans are the same
  on every run and the WAVs are byte-for-byte reproducible.
- `state.hasPanner` is `false`, i.e. the `StereoPannerNode`-less path of the prototype. The files are
  mono; `src/audio/oneshots.rs` pans them per event instead.
- Parameterised sounds are rendered once per option (`stepSprint`, `pickupRich`, `endingDawn`, …),
  and `descend` is rendered at lap 0 because the player pitches it with `PlaybackSettings::speed`.

Output format: mono 16-bit PCM at 44.1 kHz (11.025 kHz for `descend` and the three 6-second ending
chords, which are pure low sines — it saves 1.5 MB and each file's header carries its own rate).
Every file is peak-normalised to 1.0 and its original peak recorded in `manifest.ron`, so playback
restores the prototype's loudness with `volume = peak × master` (`audio.js:out(peak, pan)`).

To add or change a one-shot today, edit or replace the WAV under `undercroft/assets/audio/`, update
`manifest.ron` (file, peak, duration, samples) and keep `PLAYABLE` in
`crates/undercroft/src/audio/oneshots.rs` in step — a unit test asserts the two lists agree.
