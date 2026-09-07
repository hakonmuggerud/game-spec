//! LANE: economy. Persistence — `save.js` (DESIGN.md §11): the v3 save schema and its JSON form.
//!
//! [`SaveData`] is `save.js:defaults()` as a struct; its JSON form (`to_json` / `from_json`) is the exact
//! `localStorage` string the prototype writes under `undercroft-v2`, so a browser save loads here and a save
//! written here loads in the prototype. Loading mirrors `save.js:load` — `mergeInto` semantics (unknown keys
//! ignored, wrong types fall back to defaults), `migrateV2` (drop `explored`, drop numeric `gatesOpened`), and
//! the one-shot v1 import (`undercroft-proto` `{points, bankedOil}`). Storage itself (localStorage, a file) is
//! the app's concern: only strings cross this boundary.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use undercroft_data::config::Tier;

/// `save.js` — the schema version written (`v: 3`).
pub const SAVE_VERSION: u32 = 3;
/// `config.js:SAVE_KEY` — the v2/v3 localStorage key.
pub const SAVE_KEY: &str = "undercroft-v2";
/// `config.js:SAVE_KEY_V1` — the v1 key (imported once, mirrored for v1 tooling).
pub const SAVE_KEY_V1: &str = "undercroft-proto";
/// `config.js:AUDIO.vol` — the default master volume `defaults()` bakes in.
pub const DEFAULT_AUDIO_VOL: f32 = 0.8;

/// `save.buildings` — which hub buildings are raised (`hub.js:isBuilt`; the board is always built).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Buildings {
    pub workshop: bool,
    pub press: bool,
    pub cart: bool,
    pub shrine: bool,
    pub tram: bool,
    pub elevator: bool,
}

impl Buildings {
    /// `save.buildings[id]` — false for unknown ids (the board has no entry: `hub.js` treats it as `always`).
    pub fn get(&self, id: &str) -> bool {
        match id {
            "workshop" => self.workshop,
            "press" => self.press,
            "cart" => self.cart,
            "shrine" => self.shrine,
            "tram" => self.tram,
            "elevator" => self.elevator,
            _ => false,
        }
    }

    /// `save.buildings[id] = v`; unknown ids are ignored.
    pub fn set(&mut self, id: &str, v: bool) {
        match id {
            "workshop" => self.workshop = v,
            "press" => self.press = v,
            "cart" => self.cart = v,
            "shrine" => self.shrine = v,
            "tram" => self.tram = v,
            "elevator" => self.elevator = v,
            _ => {}
        }
    }

    /// Any building raised (`save.js:hasProgress` `any()`).
    pub fn any(&self) -> bool {
        self.workshop || self.press || self.cart || self.shrine || self.tram || self.elevator
    }
}

/// `save.rescued` — which NPCs stand in the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rescued {
    pub lamplighter: bool,
    pub cartographer: bool,
    pub keeper: bool,
    pub deacon: bool,
}

impl Rescued {
    /// The four ids in `save.js:defaults()` order.
    pub const IDS: [&'static str; 4] = ["lamplighter", "cartographer", "keeper", "deacon"];

    /// `save.rescued[id]`.
    pub fn get(&self, id: &str) -> bool {
        match id {
            "lamplighter" => self.lamplighter,
            "cartographer" => self.cartographer,
            "keeper" => self.keeper,
            "deacon" => self.deacon,
            _ => false,
        }
    }

    /// `save.rescued[id] = v`.
    pub fn set(&mut self, id: &str, v: bool) {
        match id {
            "lamplighter" => self.lamplighter = v,
            "cartographer" => self.cartographer = v,
            "keeper" => self.keeper = v,
            "deacon" => self.deacon = v,
            _ => {}
        }
    }

    /// Ids of the rescued, in table order (`endgame.js:rescuedIds`).
    pub fn ids(&self) -> Vec<&'static str> {
        Self::IDS
            .iter()
            .copied()
            .filter(|id| self.get(id))
            .collect()
    }

    /// How many are rescued.
    pub fn count(&self) -> u32 {
        self.ids().len() as u32
    }

    /// Any rescued.
    pub fn any(&self) -> bool {
        self.count() > 0
    }
}

/// `save.tools`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tools {
    pub prybar: bool,
    pub sluice: bool,
    pub censer: bool,
}

impl Tools {
    /// Is this a tool id the save knows (`id in ctx.save.tools`)?
    pub fn known(id: &str) -> bool {
        matches!(id, "prybar" | "sluice" | "censer")
    }

    /// `save.tools[id]`.
    pub fn get(&self, id: &str) -> bool {
        match id {
            "prybar" => self.prybar,
            "sluice" => self.sluice,
            "censer" => self.censer,
            _ => false,
        }
    }

    /// `save.tools[id] = v`.
    pub fn set(&mut self, id: &str, v: bool) {
        match id {
            "prybar" => self.prybar = v,
            "sluice" => self.sluice = v,
            "censer" => self.censer = v,
            _ => {}
        }
    }

    /// Any tool owned.
    pub fn any(&self) -> bool {
        self.prybar || self.sluice || self.censer
    }
}

/// `save.contracts` — the contract store `contracts.js:store()` reads and writes.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ContractSave {
    /// Accepted, not yet done (≤ `CONTRACT_CFG.maxActive`).
    pub active: Vec<String>,
    pub done: Vec<String>,
    /// Run-scoped progress per id (fractional for `survive` seconds).
    pub progress: BTreeMap<String, f64>,
}

/// `save.endings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Endings {
    pub cage: bool,
    pub dawn: bool,
    pub night: bool,
}

impl Endings {
    /// `endgame.js:ENDING_ORDER`.
    pub const IDS: [&'static str; 3] = ["cage", "dawn", "night"];

    /// `save.endings[id]`.
    pub fn get(&self, id: &str) -> bool {
        match id {
            "cage" => self.cage,
            "dawn" => self.dawn,
            "night" => self.night,
            _ => false,
        }
    }

    /// `save.endings[id] = v`.
    pub fn set(&mut self, id: &str, v: bool) {
        match id {
            "cage" => self.cage = v,
            "dawn" => self.dawn = v,
            "night" => self.night = v,
            _ => {}
        }
    }

    /// How many seen.
    pub fn count(&self) -> u32 {
        Self::IDS.iter().filter(|id| self.get(id)).count() as u32
    }

    /// Any seen.
    pub fn any(&self) -> bool {
        self.count() > 0
    }
}

/// `save.stats`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub runs: u32,
    pub deaths: u32,
    pub rescues: u32,
    /// Flame points banked over the save's life.
    pub banked: u32,
}

/// `save.audio`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSave {
    pub vol: f32,
    pub muted: bool,
}

impl Default for AudioSave {
    fn default() -> Self {
        AudioSave {
            vol: DEFAULT_AUDIO_VOL,
            muted: false,
        }
    }
}

/// The v3 save record (`save.js:defaults()`), field order = JSON key order the prototype writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SaveData {
    pub v: u32,
    /// Flame points (never spent).
    pub points: u32,
    /// Banked oil (the resource ledger, DESIGN-v2 §7).
    pub oil: u32,
    pub relics: u32,
    pub rich: u32,
    /// Workshop light-tech level (0 = none).
    pub light_tech: u32,
    /// Oil Press reservoir level (0–2).
    pub reservoir: u32,
    pub buildings: Buildings,
    pub rescued: Rescued,
    pub tools: Tools,
    /// `{zoneId: [gateId]}` — ids (the zone's tool), never cell indices.
    pub gates_opened: BTreeMap<String, Vec<String>>,
    /// `{zoneId: [shortcutId]}` — DESIGN.md §3.6.
    pub shortcuts: BTreeMap<String, Vec<String>>,
    pub contracts: ContractSave,
    pub zone_selected: String,
    /// The Shrine blessing is lit (charged at each descent).
    pub blessing: bool,
    pub endings: Endings,
    /// `{zoneId: base64 bitset}` (`hub.js:exploredBits`).
    pub explored: BTreeMap<String, String>,
    pub stats: Stats,
    pub audio: AudioSave,
    pub imported_v1: bool,
}

impl Default for SaveData {
    fn default() -> Self {
        SaveData::defaults(DEFAULT_AUDIO_VOL)
    }
}

impl SaveData {
    /// `save.js:defaults()` with the given `AUDIO.vol`.
    pub fn defaults(audio_vol: f32) -> SaveData {
        SaveData {
            v: SAVE_VERSION,
            points: 0,
            oil: 0,
            relics: 0,
            rich: 0,
            light_tech: 0,
            reservoir: 0,
            buildings: Buildings::default(),
            rescued: Rescued::default(),
            tools: Tools::default(),
            gates_opened: BTreeMap::new(),
            shortcuts: BTreeMap::new(),
            contracts: ContractSave::default(),
            zone_selected: "undercroft".to_string(),
            blessing: false,
            endings: Endings::default(),
            explored: BTreeMap::new(),
            stats: Stats::default(),
            audio: AudioSave {
                vol: audio_vol,
                muted: false,
            },
            imported_v1: false,
        }
    }

    /// `save.js:load()` — `v2_text` is the `undercroft-v2` string (or none), `v1_text` the `undercroft-proto`
    /// string (or none). A v2 record is migrated, a v3 record merged, anything else ignored; the v1 record is
    /// imported once when no known record exists. Never fails: unparsable text reads as "no record".
    pub fn load(v2_text: Option<&str>, v1_text: Option<&str>) -> SaveData {
        SaveData::load_with(v2_text, v1_text, DEFAULT_AUDIO_VOL)
    }

    /// [`SaveData::load`] with an explicit default volume (`AUDIO.vol`).
    pub fn load_with(v2_text: Option<&str>, v1_text: Option<&str>, audio_vol: f32) -> SaveData {
        let mut data = SaveData::defaults(audio_vol);
        let rec = v2_text.and_then(read_json);
        let v = rec
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|o| o.get("v"))
            .map(js_int)
            .unwrap_or(0);
        let known = v == 2 || v == 3;
        if known {
            let rec = rec.unwrap_or(Value::Null);
            let rec = if v == 2 { migrate_v2(rec) } else { rec };
            data = merge_into(&data, &rec);
        }
        data.v = SAVE_VERSION;
        if !data.imported_v1 {
            if let Some(v1) = v1_text.and_then(read_json) {
                let points = v1.get("points").and_then(Value::as_f64);
                if let (Some(p), false) = (points, known) {
                    data.points = js_int(&Value::from(p)).max(0) as u32;
                    let banked = v1.get("bankedOil").map(js_int).unwrap_or(0).max(0) as u32;
                    data.oil = data.oil.max(banked);
                }
            }
            data.imported_v1 = true;
        }
        data
    }

    /// Load a single JSON string as the prototype would read `undercroft-v2` (no v1 record).
    pub fn from_json(text: &str) -> SaveData {
        SaveData::load(Some(text), None)
    }

    /// The `undercroft-v2` string (`JSON.stringify(data)` — same keys, same order).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// The v1 mirror `save.js:flush` also writes: `{points, bankedOil}`.
    pub fn to_json_v1(&self) -> String {
        format!(r#"{{"points":{},"bankedOil":{}}}"#, self.points, self.oil)
    }

    /// `save.js:reset()` — restore defaults in place (the record keeps its identity) with `importedV1 = true`.
    /// The JS resets `audio` too (`defaults()` re-reads `AUDIO.vol`); `keep_audio` preserves the current volume
    /// and mute, which is what the running audio module expects.
    pub fn reset(&mut self, keep_audio: bool) {
        let audio = self.audio;
        *self = SaveData::default();
        if keep_audio {
            self.audio = audio;
        }
        self.imported_v1 = true;
    }

    /// `save.js:hasProgress` — anything worth a "Continue".
    pub fn has_progress(&self) -> bool {
        self.stats.runs > 0
            || self.points > 0
            || self.oil > 0
            || self.relics > 0
            || self.rich > 0
            || self.light_tech > 0
            || self.buildings.any()
            || self.rescued.any()
            || self.tools.any()
            || self.endings.any()
            || !self.contracts.active.is_empty()
            || !self.contracts.done.is_empty()
    }

    /// `save.js:summary` — the one-line save summary under "Continue".
    pub fn summary(&self, tiers: &[Tier]) -> SaveSummary {
        let pts = self.points;
        let mut tier = 1;
        for (i, t) in tiers.iter().enumerate() {
            if pts >= t.pts {
                tier = i as u32 + 1;
            }
        }
        let s = SaveSummary {
            tier,
            points: pts,
            rescued: self.rescued.count(),
            rescued_total: Rescued::IDS.len() as u32,
            endings: self.endings.count(),
            endings_total: Endings::IDS.len() as u32,
            runs: self.stats.runs,
            text: String::new(),
        };
        SaveSummary {
            text: format!(
                "Flame tier {} · {}/{} rescued · {}/{} endings seen",
                s.tier, s.rescued, s.rescued_total, s.endings, s.endings_total
            ),
            ..s
        }
    }

    /// `save.gatesOpened[zone]` holds this gate id.
    pub fn gate_opened(&self, zone: &str, id: &str) -> bool {
        self.gates_opened
            .get(zone)
            .map(|l| l.iter().any(|g| g == id))
            .unwrap_or(false)
    }

    /// Record an opened gate (idempotent).
    pub fn open_gate(&mut self, zone: &str, id: &str) {
        let l = self.gates_opened.entry(zone.to_string()).or_default();
        if !l.iter().any(|g| g == id) {
            l.push(id.to_string());
        }
    }

    /// `save.shortcuts[zone]` holds this shortcut id.
    pub fn shortcut_opened(&self, zone: &str, id: &str) -> bool {
        self.shortcuts
            .get(zone)
            .map(|l| l.iter().any(|g| g == id))
            .unwrap_or(false)
    }

    /// Record a lifted shortcut (idempotent).
    pub fn open_shortcut(&mut self, zone: &str, id: &str) {
        let l = self.shortcuts.entry(zone.to_string()).or_default();
        if !l.iter().any(|g| g == id) {
            l.push(id.to_string());
        }
    }

    /// `hub.js:exploredBits` — the explored bitset of a zone with `n_cells` cells (1 bit per cell, `idx = cz·w +
    /// cx`), decoded from `save.explored[zone]`. A stored bitset whose byte length does not match this grid was
    /// written for another map size and is dropped rather than stretched (DESIGN.md §3.3).
    pub fn explored_bits(&self, zone: &str, n_cells: usize) -> Vec<u8> {
        let n = n_cells.div_ceil(8);
        match self.explored.get(zone).and_then(|s| decode_bits(s)) {
            Some(bits) if bits.len() == n => bits,
            _ => vec![0; n],
        }
    }

    /// `hub.js:flushExplored` for one zone — store the bitset as base64.
    pub fn set_explored_bits(&mut self, zone: &str, bits: &[u8]) {
        self.explored.insert(zone.to_string(), encode_bits(bits));
    }
}

/// `save.js:summary()` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSummary {
    pub tier: u32,
    pub points: u32,
    pub rescued: u32,
    pub rescued_total: u32,
    pub endings: u32,
    pub endings_total: u32,
    pub runs: u32,
    pub text: String,
}

/* ============================================================
Explored bitsets (hub.js b64 / unb64 / bitSet)
============================================================ */

/// `hub.js:b64` — bytes → base64.
pub fn encode_bits(bits: &[u8]) -> String {
    B64.encode(bits)
}

/// `hub.js:unb64` — base64 → bytes; `None` when the string is not valid base64 (the JS falls back to a fresh
/// bitset, and so does [`SaveData::explored_bits`]).
pub fn decode_bits(s: &str) -> Option<Vec<u8>> {
    B64.decode(s).ok()
}

/// `hub.js:bitSet` — is cell `i` marked?
pub fn bit_set(bits: &[u8], i: usize) -> bool {
    bits.get(i >> 3)
        .map(|b| b & (1 << (i & 7)) != 0)
        .unwrap_or(false)
}

/// `hub.js:exploreTick` `mark()` — set cell `i`; returns true when it was newly set.
pub fn mark_bit(bits: &mut [u8], i: usize) -> bool {
    match bits.get_mut(i >> 3) {
        Some(b) => {
            let m = 1 << (i & 7);
            if *b & m == 0 {
                *b |= m;
                true
            } else {
                false
            }
        }
        None => false,
    }
}

/* ============================================================
mergeInto / migrateV2 on the JSON tree
============================================================ */

/// `save.js:readKey` — parse, or nothing on any error.
fn read_json(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok()
}

/// JS `x | 0` on a JSON value: numbers truncate toward zero (NaN/∞ → 0), anything else is 0.
fn js_int(v: &Value) -> i64 {
    match v.as_f64() {
        Some(f) if f.is_finite() => (f.trunc() as i64).clamp(i32::MIN as i64, i32::MAX as i64),
        _ => 0,
    }
}

/// JS truthiness of a JSON value (`!!s`).
fn js_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// The keys `save.js:mergeInto` copies wholesale (open-keyed maps) instead of recursing into.
const WHOLESALE: [&str; 4] = ["gatesOpened", "shortcuts", "explored", "progress"];

/// `save.js:mergeInto(dst, src)` on JSON trees: keep `dst`'s shape, copy what `src` has of the right type.
fn merge_value(dst: &mut Value, src: &Value) {
    let src_obj = match src.as_object() {
        Some(o) => o,
        None => return,
    };
    let dst_obj = match dst.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    for (k, d) in dst_obj.iter_mut() {
        let s = match src_obj.get(k) {
            Some(s) => s,
            None => continue,
        };
        match d {
            Value::Object(_) => {
                if WHOLESALE.contains(&k.as_str()) {
                    if s.is_object() {
                        *d = s.clone();
                    }
                } else {
                    merge_value(d, s);
                }
            }
            Value::Array(_) => {
                if s.is_array() {
                    *d = s.clone();
                }
            }
            Value::Number(_) => {
                if s.as_f64().map(f64::is_finite).unwrap_or(false) {
                    *d = s.clone();
                }
            }
            Value::Bool(_) => *d = Value::Bool(js_truthy(s)),
            Value::String(_) => {
                if s.is_string() {
                    *d = s.clone();
                }
            }
            Value::Null => {}
        }
    }
}

/// Coerce the wholesale-copied and array-valued parts back to the shapes the typed struct needs, the way the
/// JS consumers tolerate them: string lists keep their strings, string maps keep their strings, `progress`
/// keeps finite numbers, unsigned counters are `| 0` and clamped at 0.
fn sanitize(v: &mut Value) {
    let o = match v.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    for k in ["gatesOpened", "shortcuts"] {
        if let Some(Value::Object(m)) = o.get_mut(k) {
            let mut out = Map::new();
            for (zone, list) in m.iter() {
                if let Some(arr) = list.as_array() {
                    let keep: Vec<Value> = arr.iter().filter(|e| e.is_string()).cloned().collect();
                    out.insert(zone.clone(), Value::Array(keep));
                }
            }
            *m = out;
        }
    }
    if let Some(Value::Object(m)) = o.get_mut("explored") {
        m.retain(|_, e| e.is_string());
    }
    if let Some(Value::Object(c)) = o.get_mut("contracts") {
        for k in ["active", "done"] {
            if let Some(Value::Array(a)) = c.get_mut(k) {
                a.retain(|e| e.is_string());
            }
        }
        if let Some(Value::Object(p)) = c.get_mut("progress") {
            p.retain(|_, e| e.as_f64().map(f64::is_finite).unwrap_or(false));
        }
    }
    for k in [
        "v",
        "points",
        "oil",
        "relics",
        "rich",
        "lightTech",
        "reservoir",
    ] {
        if let Some(n) = o.get_mut(k) {
            *n = Value::from(js_int(n).max(0));
        }
    }
    if let Some(Value::Object(st)) = o.get_mut("stats") {
        for (_, n) in st.iter_mut() {
            *n = Value::from(js_int(n).max(0));
        }
    }
}

/// `save.js:mergeInto(defaults, rec)` typed: the merged record, or `dst` untouched when `rec` is unusable.
pub fn merge_into(dst: &SaveData, rec: &Value) -> SaveData {
    let mut tree = match serde_json::to_value(dst) {
        Ok(t) => t,
        Err(_) => return dst.clone(),
    };
    merge_value(&mut tree, rec);
    sanitize(&mut tree);
    serde_json::from_value(tree).unwrap_or_else(|_| dst.clone())
}

/// `save.js:migrateV2(rec)` — a v3-shaped copy of a v2 record: `v: 3`, `explored` dropped, numeric
/// `gatesOpened` entries dropped (zones left empty are removed), `shortcuts` an object.
pub fn migrate_v2(rec: Value) -> Value {
    let mut out = match rec {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    out.insert("v".into(), Value::from(3));
    out.insert("explored".into(), Value::Object(Map::new()));
    let mut go = Map::new();
    if let Some(Value::Object(m)) = out.get("gatesOpened") {
        for (zone, list) in m {
            if let Some(arr) = list.as_array() {
                let keep: Vec<Value> = arr.iter().filter(|e| e.is_string()).cloned().collect();
                if !keep.is_empty() {
                    go.insert(zone.clone(), Value::Array(keep));
                }
            }
        }
    }
    out.insert("gatesOpened".into(), Value::Object(go));
    if !out.get("shortcuts").map(Value::is_object).unwrap_or(false) {
        out.insert("shortcuts".into(), Value::Object(Map::new()));
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_save_js() {
        let d = SaveData::default();
        assert_eq!(d.v, 3);
        assert_eq!(d.zone_selected, "undercroft");
        assert_eq!(d.audio.vol, 0.8);
        assert!(!d.imported_v1);
        assert!(!d.has_progress());
        let json = d.to_json();
        // key set and order = defaults() insertion order
        let tree: Value = serde_json::from_str(&json).expect("json");
        assert_eq!(tree.as_object().map(|o| o.len()), Some(20));
        assert!(json.starts_with(r#"{"v":3,"points":0,"oil":0,"relics":0,"rich":0,"lightTech":0,"reservoir":0,"buildings":{"workshop":false"#));
        assert!(json.ends_with(r#""audio":{"vol":0.8,"muted":false},"importedV1":false}"#));
    }

    #[test]
    fn round_trip() {
        let mut d = SaveData {
            points: 17,
            oil: 120,
            ..Default::default()
        };
        d.buildings.tram = true;
        d.rescued.set("deacon", true);
        d.tools.prybar = true;
        d.open_gate("undercroft", "prybar");
        d.open_shortcut("undercroft", "u_rood");
        d.contracts.active.push("c_wick".into());
        d.contracts.done.push("c_relight".into());
        d.contracts.progress.insert("c_wick".into(), 2.0);
        d.contracts.progress.insert("c_sound".into(), 12.5);
        d.set_explored_bits("undercroft", &[1, 2, 255]);
        d.endings.night = true;
        d.stats.runs = 4;
        d.audio.muted = true;
        d.imported_v1 = true;
        let back = SaveData::from_json(&d.to_json());
        assert_eq!(back, d);
        assert_eq!(back.explored_bits("undercroft", 24), vec![1, 2, 255]);
        assert_eq!(back.explored_bits("undercroft", 40), vec![0; 5]); // size mismatch → fresh
        assert!(back.gate_opened("undercroft", "prybar"));
        assert!(back.shortcut_opened("undercroft", "u_rood"));
        assert_eq!(back.to_json_v1(), r#"{"points":17,"bankedOil":120}"#);
    }

    /// A localStorage string as the prototype writes it (save.js:14-30 shape, v3, typical mid-game values).
    const PROTO_V3: &str = r#"{"v":3,"points":9,"oil":85,"relics":3,"rich":1,"lightTech":1,"reservoir":1,
        "buildings":{"workshop":true,"press":false,"cart":false,"shrine":false,"tram":true,"elevator":false},
        "rescued":{"lamplighter":true,"cartographer":false,"keeper":false,"deacon":false},
        "tools":{"prybar":true,"sluice":false,"censer":false},
        "gatesOpened":{"undercroft":["prybar"]},"shortcuts":{"undercroft":["u_rood","u_west"]},
        "contracts":{"active":["c_wick"],"done":["c_relight"],"progress":{"c_relight":1,"c_wick":0}},
        "zoneSelected":"cistern","blessing":false,"endings":{"cage":false,"dawn":false,"night":false},
        "explored":{"undercroft":"AQID"},"stats":{"runs":5,"deaths":2,"rescues":1,"banked":9},
        "audio":{"vol":0.6,"muted":false},"importedV1":true}"#;

    #[test]
    fn loads_a_prototype_localstorage_record() {
        let d = SaveData::from_json(PROTO_V3);
        assert_eq!(d.points, 9);
        assert_eq!(d.oil, 85);
        assert_eq!(d.light_tech, 1);
        assert_eq!(d.reservoir, 1);
        assert!(d.buildings.workshop && d.buildings.tram && !d.buildings.press);
        assert!(d.rescued.lamplighter && !d.rescued.deacon);
        assert!(d.tools.prybar);
        assert_eq!(d.gates_opened["undercroft"], vec!["prybar"]);
        assert_eq!(d.shortcuts["undercroft"], vec!["u_rood", "u_west"]);
        assert_eq!(d.contracts.active, vec!["c_wick"]);
        assert_eq!(d.contracts.done, vec!["c_relight"]);
        assert_eq!(d.contracts.progress["c_relight"], 1.0);
        assert_eq!(d.zone_selected, "cistern");
        assert_eq!(d.explored_bits("undercroft", 24), vec![1, 2, 3]);
        assert_eq!(d.stats.runs, 5);
        assert_eq!(d.audio.vol, 0.6);
        assert!(d.imported_v1);
        assert!(d.has_progress());
        let tiers = vec![
            Tier {
                pts: 0,
                int: 2.0,
                dist: 7.0,
                msg: String::new(),
            },
            Tier {
                pts: 6,
                int: 3.5,
                dist: 11.0,
                msg: String::new(),
            },
            Tier {
                pts: 15,
                int: 5.5,
                dist: 16.0,
                msg: String::new(),
            },
            Tier {
                pts: 30,
                int: 8.0,
                dist: 24.0,
                msg: String::new(),
            },
        ];
        let s = d.summary(&tiers);
        assert_eq!(s.tier, 2);
        assert_eq!(s.text, "Flame tier 2 · 1/4 rescued · 0/3 endings seen");
    }

    #[test]
    fn merge_ignores_unknown_keys_and_wrong_types() {
        let text = r#"{"v":3,"points":"twelve","oil":40.7,"relics":-3,"bogus":1,"blessing":1,
            "buildings":{"workshop":"yes","nope":true},"rescued":[1,2],"zoneSelected":7,
            "gatesOpened":{"undercroft":["prybar",5,null],"cistern":"x"},"shortcuts":[],
            "contracts":{"active":["c_wick",3],"done":"no","progress":{"c_wick":"a","c_sound":4.5}},
            "explored":{"undercroft":12,"cistern":"AQ=="},"stats":{"runs":"9","deaths":2.9},"audio":{"vol":"loud","muted":"no"}}"#;
        let d = SaveData::from_json(text);
        assert_eq!(d.points, 0); // wrong type → default
        assert_eq!(d.oil, 40); // finite number kept, |0
        assert_eq!(d.relics, 0); // negative clamped
        assert!(d.blessing); // !!1
        assert!(d.buildings.workshop); // !!"yes"
        assert_eq!(d.rescued, Rescued::default()); // array is not an object → untouched
        assert_eq!(d.zone_selected, "undercroft");
        assert_eq!(d.gates_opened["undercroft"], vec!["prybar"]);
        assert!(!d.gates_opened.contains_key("cistern"));
        assert!(d.shortcuts.is_empty()); // array is not an object
        assert_eq!(d.contracts.active, vec!["c_wick"]);
        assert!(d.contracts.done.is_empty());
        assert_eq!(d.contracts.progress.len(), 1);
        assert_eq!(d.contracts.progress["c_sound"], 4.5);
        assert_eq!(d.explored.len(), 1);
        assert_eq!(d.stats.runs, 0);
        assert_eq!(d.stats.deaths, 2);
        assert_eq!(d.audio.vol, 0.8);
        assert!(d.audio.muted); // !!"no" is true in JS
        assert!(d.imported_v1); // load() marks it
    }

    #[test]
    fn unknown_versions_and_garbage_read_as_fresh() {
        assert!(SaveData::from_json("not json").imported_v1);
        let fresh = SaveData::from_json(r#"{"v":1,"points":50}"#);
        assert_eq!(fresh.points, 0);
        let fresh = SaveData::from_json(r#"{"v":9,"points":50}"#);
        assert_eq!(fresh.points, 0);
        let fresh = SaveData::from_json("[1,2]");
        assert_eq!(fresh.points, 0);
    }

    #[test]
    fn migrate_v2_drops_explored_and_numeric_gates() {
        let v2 = r#"{"v":2,"points":12,"oil":30,"relics":2,"rich":0,"lightTech":0,"reservoir":0,
            "buildings":{"workshop":true},"rescued":{"lamplighter":true},"tools":{"prybar":true},
            "gatesOpened":{"undercroft":[1234,"prybar"],"cistern":[77]},
            "contracts":{"active":[],"done":["c_relight"],"progress":{"c_relight":1}},
            "zoneSelected":"undercroft","blessing":false,"endings":{"cage":true},
            "explored":{"undercroft":"AAAA"},"stats":{"runs":3,"deaths":1,"rescues":1,"banked":12},
            "audio":{"vol":0.8,"muted":false},"importedV1":true}"#;
        let d = SaveData::from_json(v2);
        assert_eq!(d.v, 3);
        assert_eq!(d.points, 12);
        assert!(d.buildings.workshop);
        assert!(d.rescued.lamplighter);
        assert!(d.tools.prybar);
        assert_eq!(d.contracts.done, vec!["c_relight"]);
        assert!(d.endings.cage);
        assert_eq!(d.stats.banked, 12);
        assert!(d.explored.is_empty(), "explored dropped");
        assert_eq!(d.gates_opened["undercroft"], vec!["prybar"]);
        assert!(
            !d.gates_opened.contains_key("cistern"),
            "numeric-only zone dropped"
        );
        assert!(d.shortcuts.is_empty());
        // the migrated tree itself
        let m = migrate_v2(serde_json::from_str(v2).unwrap());
        assert_eq!(m["v"], 3);
        assert_eq!(m["explored"], serde_json::json!({}));
        assert_eq!(m["shortcuts"], serde_json::json!({}));
    }

    #[test]
    fn v1_import_once() {
        let v1 = r#"{"points":7,"bankedOil":55}"#;
        let d = SaveData::load(None, Some(v1));
        assert_eq!(d.points, 7);
        assert_eq!(d.oil, 55);
        assert!(d.imported_v1);
        // a known v3 record wins over the v1 one, and importedV1 flips regardless
        let d = SaveData::load(
            Some(r#"{"v":3,"points":2,"oil":10,"importedV1":false}"#),
            Some(v1),
        );
        assert_eq!(d.points, 2);
        assert_eq!(d.oil, 10);
        assert!(d.imported_v1);
        // already imported: the v1 record is ignored
        let d = SaveData::load(Some(r#"{"v":3,"points":2,"importedV1":true}"#), Some(v1));
        assert_eq!(d.points, 2);
        // negative v1 points clamp to 0
        let d = SaveData::load(None, Some(r#"{"points":-4,"bankedOil":-9}"#));
        assert_eq!(d.points, 0);
        assert_eq!(d.oil, 0);
    }

    #[test]
    fn reset_and_bits() {
        let mut d = SaveData::from_json(PROTO_V3);
        d.reset(true);
        assert_eq!(d.points, 0);
        assert_eq!(d.audio.vol, 0.6);
        assert!(d.imported_v1);
        d.reset(false);
        assert_eq!(d.audio.vol, 0.8);
        let mut bits = vec![0u8; 3];
        assert!(mark_bit(&mut bits, 9));
        assert!(!mark_bit(&mut bits, 9));
        assert!(bit_set(&bits, 9));
        assert!(!bit_set(&bits, 8));
        assert!(!bit_set(&bits, 99));
        assert!(!mark_bit(&mut bits, 99));
        assert_eq!(decode_bits(&encode_bits(&bits)), Some(bits.clone()));
        assert_eq!(decode_bits("!!!"), None);
    }
}
