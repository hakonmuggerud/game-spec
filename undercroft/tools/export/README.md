# tools/export

`export.mjs` imports the prototype's ES modules straight from `../../../prototype/src` (read-only) and writes:

- `out/*.json` — every data table (`config`, `palettes`, `zones`, `contracts`, `npcs`, `buildings`, `endgame`,
  `models`) with the snake_case keys of the Rust structs in `crates/undercroft-data`;
- `../../assets/data/maps/*.txt` — the ASCII rows of the four zones and the v2 hub;
- `../../assets/fixtures/*.json` — the parity fixtures (format: `assets/fixtures/README.md`).

`hooks.mjs` is a `module.register` resolve hook that redirects the bare `three` specifier to the copy installed
here, so `models.js` loads without touching `prototype/`.

```sh
npm install                # three@0.160.0 (the version the prototype's importmap pins)
node export.mjs            # or: npm run export
cd ../.. && cargo run -p undercroft-data --bin json2ron    # JSON → typed structs → assets/data/*.ron
```

Commit the RON, the map `.txt` files and the fixtures; `out/` and `node_modules/` are ignored.
