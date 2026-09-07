//! LANE: economy. Persistence — `save.js` (DESIGN.md §11): the v2 save schema and its JSON form.
//!
//! Must contain: `SaveData` (points, oil, relics, rich, buildings, rescued, lightTech, tools, contracts,
//! gatesOpened, shortcuts, explored (base64 bitsets per zone), endings, stats, reservoir, blessing, sound
//! settings — read `save.js` for the exact key set and defaults), `SaveData::default_new()`, the v1 import
//! (`SAVE_KEY_V1`), `to_json` / `from_json` that round-trip the prototype's `localStorage` strings byte-for-byte
//! in meaning (serde_json), `clear` (reset in place, sound kept) and the base64 explored-bitset helpers.
//! Storage itself (localStorage / a file) is the app's concern. Nothing else.
