# reference/tools/export — historical

This is the data exporter that regenerated `undercroft/assets/data/*.ron`, the ASCII maps and the
parity fixtures **from the JS prototype during the port**. That flow is retired: the RON under
`undercroft/assets/data/` is now the source of truth and is edited directly, and the `json2ron`
binary that turned this tool's JSON into RON no longer exists. Keep this directory as a record of
how the numbers were obtained; do not run it expecting it to update the game.

What it did: `export.mjs` imported the prototype's ES modules straight from
`../../prototype/src` (read-only) and wrote

- `out/*.json` — every data table (`config`, `palettes`, `zones`, `contracts`, `npcs`, `buildings`,
  `endgame`, `models`) with the snake_case keys of the Rust structs in `crates/undercroft-data`;
- `../../../undercroft/assets/data/maps/*.txt` — the ASCII rows of the four zones and the v2 hub;
- `../../../undercroft/assets/fixtures/*.json` — the parity fixtures (format:
  `undercroft/assets/fixtures/README.md`), which the sim tests still read as frozen golden data.

`hooks.mjs` is a `module.register` resolve hook that redirects the bare `three` specifier to the
copy installed here, so `models.js` loads without touching `reference/prototype/`.

`audio/` is the one-shot renderer that baked `undercroft/assets/audio/*.wav` + `manifest.ron` from
`audio.js`; the WAVs are committed and are source now too. See `audio/README.md`.

```sh
npm install                # three@0.160.0 (the version the prototype's importmap pins)
node export.mjs            # still runs; overwrites the maps and fixtures, writes out/*.json
```
