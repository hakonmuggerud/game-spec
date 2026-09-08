//! Engine-neutral game data for The Undercroft: the shared types mirroring `prototype/src/*.js`, the RON and
//! text-map loaders and the map parser. Depends on serde only; never on Bevy.
//!
//! Data flow: `tools/export/export.mjs` evaluates the prototype's ES modules and dumps JSON; the `json2ron`
//! binary deserialises that JSON into these types and writes `assets/data/*.ron`, so the committed RON is
//! guaranteed to match the Rust types. `GameData::from_dir` loads the RON plus the ASCII maps.
//!
//! Modules: [`cell`] (cell kinds + legend), [`config`] (`config.js`), [`zone`] (`maps/<id>.js` + `TUNING` +
//! `PALETTES`), [`tables`] (contracts, NPCs, buildings, endings), [`models`] (voxel boxes), [`map`] (the parser).

pub mod cell;
pub mod config;
pub mod map;
pub mod models;
pub mod tables;
pub mod zone;

pub use cell::{legend, CellKind, CreatureKind, Legend};
pub use config::Config;
pub use map::{parse_hub, parse_map, parse_zone, MapError, Marker, ParsedMap};
pub use models::{BoxDef, ModelDef, ModelTable, PartDef};
pub use tables::{
    BuildingDef, BuildingTable, ContractDef, ContractKind, ContractTable, EndgameData, EndingDef,
    EndingLine, ItemKind, NpcDef, NpcTable,
};
pub use zone::{
    Anchor, Bands, DeepStyle, EntryKind, Facing, Palette, Region, ShortcutDef, ZoneDef, ZONE_ORDER,
};

use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Schema version of the data files this crate understands.
pub const DATA_VERSION: u32 = 1;

/// Loader failure.
#[derive(Debug, Error)]
pub enum DataError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Ron {
        path: PathBuf,
        #[source]
        source: ron::error::SpannedError,
    },
    #[error("{path}: {source}")]
    Map {
        path: PathBuf,
        #[source]
        source: MapError,
    },
    #[error("{path}: map rows are {rows} lines but the zone size is {size}")]
    Size {
        path: PathBuf,
        rows: usize,
        size: i32,
    },
    /// A caller-supplied reader ([`GameData::from_reader`]) could not produce the file. `path` is the
    /// data-directory-relative name that was asked for ("config.ron", "maps/hub.txt").
    #[error("{path}: {message}")]
    Read { path: String, message: String },
}

/// Read a RON file into `T`.
pub fn load_ron<T: DeserializeOwned>(path: &Path) -> Result<T, DataError> {
    let text = std::fs::read_to_string(path).map_err(|source| DataError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    ron::from_str(&text).map_err(|source| DataError::Ron {
        path: path.to_path_buf(),
        source,
    })
}

/// Read an ASCII map (`assets/data/maps/<id>.txt`): one row per line, blank trailing lines ignored.
pub fn load_rows(path: &Path) -> Result<Vec<String>, DataError> {
    let text = std::fs::read_to_string(path).map_err(|source| DataError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(text
        .lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Everything the game reads from `assets/data/`.
#[derive(Debug, Clone, PartialEq)]
pub struct GameData {
    /// `config.ron`.
    pub config: Config,
    /// `palettes.ron` — `maps.js:PALETTES` (includes `hub`).
    pub palettes: BTreeMap<String, Palette>,
    /// `zones.ron` in `ZONE_ORDER`, rows attached from `maps/<id>.txt`.
    pub zones: Vec<ZoneDef>,
    /// `maps/hub.txt` — `maps.js:HUB_ROWS_V2`.
    pub hub_rows: Vec<String>,
    /// `contracts.ron`.
    pub contracts: ContractTable,
    /// `npcs.ron`.
    pub npcs: NpcTable,
    /// `buildings.ron`.
    pub buildings: BuildingTable,
    /// `endgame.ron`.
    pub endgame: EndgameData,
    /// `models.ron`.
    pub models: ModelTable,
}

impl GameData {
    /// The RON file names under the data directory (no extension), in load order.
    pub const FILES: [&'static str; 8] = [
        "config",
        "palettes",
        "zones",
        "contracts",
        "npcs",
        "buildings",
        "endgame",
        "models",
    ];

    /// Load `assets/data/` (`dir` is that directory). A thin wrapper over [`GameData::from_reader`]
    /// that reads each name with `std::fs`.
    pub fn from_dir(dir: &Path) -> Result<GameData, DataError> {
        let mut read = |rel: &str| -> Result<String, String> {
            std::fs::read_to_string(dir.join(rel)).map_err(|e| e.to_string())
        };
        GameData::from_reader(&mut read)
    }

    /// Load the same eight RON files and five map texts through a caller-supplied reader, so a host that
    /// does not have a filesystem (the Bevy asset server, a wasm build) can provide the bytes. `read` is
    /// called with data-directory-relative names: `"config.ron"`, `"maps/hub.txt"`. Load order and every
    /// validation are identical to [`GameData::from_dir`].
    pub fn from_reader(
        read: &mut dyn FnMut(&str) -> Result<String, String>,
    ) -> Result<GameData, DataError> {
        fn text(
            read: &mut dyn FnMut(&str) -> Result<String, String>,
            name: &str,
        ) -> Result<String, DataError> {
            read(name).map_err(|message| DataError::Read {
                path: name.to_string(),
                message,
            })
        }
        fn ron_file<T: DeserializeOwned>(
            read: &mut dyn FnMut(&str) -> Result<String, String>,
            stem: &str,
        ) -> Result<T, DataError> {
            let name = format!("{stem}.ron");
            let src = text(read, &name)?;
            ron::from_str(&src).map_err(|source| DataError::Ron {
                path: PathBuf::from(name),
                source,
            })
        }
        fn rows(
            read: &mut dyn FnMut(&str) -> Result<String, String>,
            name: &str,
        ) -> Result<Vec<String>, DataError> {
            let src = text(read, name)?;
            Ok(src
                .lines()
                .map(|l| l.trim_end_matches('\r').to_string())
                .filter(|l| !l.is_empty())
                .collect())
        }

        let config: Config = ron_file(read, "config")?;
        let palettes = ron_file(read, "palettes")?;
        let mut zones: Vec<ZoneDef> = ron_file(read, "zones")?;
        for z in &mut zones {
            let name = format!("maps/{}.txt", z.id);
            let map_rows = rows(read, &name)?;
            if map_rows.len() != z.size as usize {
                return Err(DataError::Size {
                    path: PathBuf::from(name),
                    rows: map_rows.len(),
                    size: z.size,
                });
            }
            // parse once so a bad map fails at load time, not in the sim
            let mut probe = z.clone();
            probe.rows = map_rows.clone();
            parse_zone(&probe).map_err(|source| DataError::Map {
                path: PathBuf::from(name),
                source,
            })?;
            z.rows = map_rows;
        }
        let hub_rows = rows(read, "maps/hub.txt")?;
        parse_hub(&hub_rows, config.hub_ox).map_err(|source| DataError::Map {
            path: PathBuf::from("maps/hub.txt"),
            source,
        })?;
        Ok(GameData {
            config,
            palettes,
            zones,
            hub_rows,
            contracts: ron_file(read, "contracts")?,
            npcs: ron_file(read, "npcs")?,
            buildings: ron_file(read, "buildings")?,
            endgame: ron_file(read, "endgame")?,
            models: ron_file(read, "models")?,
        })
    }

    /// The repository's `assets/data` directory, for tests and tools built from the workspace.
    pub fn workspace_data_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data")
    }

    /// The repository's `assets/fixtures` directory (parity fixtures from the exporter).
    pub fn workspace_fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fixtures")
    }

    /// Zone by id.
    pub fn zone(&self, id: &str) -> Option<&ZoneDef> {
        self.zones.iter().find(|z| z.id == id)
    }

    /// `maps.js:parseZone(id)`.
    pub fn parse_zone(&self, id: &str) -> Option<Result<ParsedMap, MapError>> {
        self.zone(id).map(parse_zone)
    }

    /// `maps.js:parseHub()` on the v2 rows.
    pub fn parse_hub(&self) -> Result<ParsedMap, MapError> {
        parse_hub(&self.hub_rows, self.config.hub_ox)
    }

    /// `maps.js:zoneLocked(id, save, tier)` reduced to its inputs: `None` when the zone can be entered, else the
    /// reason string ("Needs the Tram dock and Light-tech II").
    pub fn zone_locked(
        &self,
        id: &str,
        buildings_built: &dyn Fn(&str) -> bool,
        light_tech: u32,
        tier: u32,
        rescued: &dyn Fn(&str) -> bool,
    ) -> Option<String> {
        let z = match self.zone(id) {
            Some(z) => z,
            None => return Some("Unknown zone".to_string()),
        };
        let r = z.requires.as_ref()?;
        let mut missing = Vec::new();
        if let Some(b) = &r.building {
            if !buildings_built(b) {
                missing.push(if b == "tram" {
                    "the Tram dock".to_string()
                } else {
                    "the Elevator".to_string()
                });
            }
        }
        if let Some(lt) = r.light_tech {
            if light_tech < lt {
                missing.push(format!("Light-tech {}", "I".repeat(lt as usize)));
            }
        }
        if let Some(t) = r.tier {
            if tier < t {
                missing.push(format!("flame tier {t}"));
            }
        }
        if let Some(n) = &r.rescued {
            if !rescued(n) {
                missing.push(if n == "deacon" {
                    "Deacon Maud".to_string()
                } else {
                    n.clone()
                });
            }
        }
        if missing.is_empty() {
            None
        } else {
            Some(format!("Needs {}", missing.join(" and ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    #[test]
    fn from_reader_matches_from_dir() {
        let dir = GameData::workspace_data_dir();
        let mut asked: Vec<String> = Vec::new();
        let mut read = |rel: &str| -> Result<String, String> {
            asked.push(rel.to_string());
            std::fs::read_to_string(dir.join(rel)).map_err(|e| e.to_string())
        };
        let via_reader = GameData::from_reader(&mut read).expect("from_reader loads");
        assert_eq!(via_reader, data());
        assert_eq!(asked.len(), GameData::FILES.len() + 5);
        assert!(asked.contains(&"config.ron".to_string()));
        assert!(asked.contains(&"maps/hub.txt".to_string()));
        let err = GameData::from_reader(&mut |_| Err("nope".into())).unwrap_err();
        assert_eq!(err.to_string(), "config.ron: nope");
    }

    #[test]
    fn loads_and_spot_checks_config() {
        let d = data();
        let c = &d.config;
        assert_eq!(c.hub_ox, 60);
        assert_eq!(c.cfg.lamp_dist, 11.0);
        assert_eq!(c.cfg.walk, 2.6);
        assert_eq!(c.cfg.start_oil, [45.0, 50.0, 55.0, 60.0]);
        assert_eq!(c.cfg.lamp_color, 0xffb265);
        assert_eq!(c.cfg.band_burn.at(3), 1.1 + 0.12 * 3.0);
        assert!((c.cfg.band_lamp.at(5) - (0.95 - 0.08 * 5.0)).abs() < 1e-6);
        assert_eq!(c.hunter.lamp_r, 12.0);
        assert_eq!(c.hunter_profiles["base"].speed["CHASE"], 3.6);
        assert_eq!(c.hunter_profiles["fast"].scale_y, 1.15);
        assert!(!c.hunter_profiles["lampwight"].kills);
        assert!(c.hunter_profiles["base"].kills);
        let brute = c.hunter_profiles["brute"].senses.as_ref().expect("senses");
        assert_eq!(brute.walk, 1.5);
        assert!(brute.follower);
        assert_eq!(
            c.hunter_profiles["falseLight"]
                .senses
                .as_ref()
                .and_then(|s| s.proximity),
            Some(3.0)
        );
        assert_eq!(c.creature.warden.light.int["ALERT"], 3.0);
        assert_eq!(c.creature.drowner.drift_t, [3.0, 6.0]);
        assert_eq!(c.tiers.len(), 4);
        assert_eq!(c.tiers[3].pts, 30);
        assert_eq!(c.light_tech[3].cost.rich, 2);
        assert_eq!(c.build_costs.elevator.relics, Some(4));
        assert_eq!(c.build_costs.tram.tier, Some(2));
        assert_eq!(c.keys["sprint"], vec!["ShiftLeft", "ShiftRight"]);
        assert_eq!(c.tools["prybar"], "Pry Bar");
        assert_eq!(c.hub_block.flame, 8);
        assert_eq!(c.embers.per_tier, [18, 36, 60, 90]);
        assert_eq!(c.npc_cfg.save_r, 4.0);
        assert_eq!(c.contract_cfg.max_active, 2);
        assert_eq!(c.endgame.extra_laps, vec![3, 4]);
    }

    #[test]
    fn zones_match_the_prototype() {
        let d = data();
        assert_eq!(d.zones.len(), 4);
        let ids: Vec<&str> = d.zones.iter().map(|z| z.id.as_str()).collect();
        assert_eq!(ids, ZONE_ORDER);
        let u = d.zone("undercroft").expect("undercroft");
        assert_eq!(u.size, 62);
        assert_eq!(u.rows.len(), 62);
        assert!(u.rows.iter().all(|r| r.len() == 62));
        assert_eq!(u.regions.len(), 13);
        assert_eq!(u.shortcuts.len(), 3);
        assert_eq!(u.anchor("entry"), Some([31, 58]));
        assert_eq!(u.anchor("shortcuts.u_rood"), Some([53, 36]));
        // maps.js:speeds — an unknown profile name resolves to `base`
        assert_eq!(
            u.hunter_profile_names(&d.config),
            u.hunters.iter().map(String::as_str).collect::<Vec<_>>()
        );
        let mut typo = u.clone();
        typo.hunters = vec!["fast".into(), "fastt".into()];
        assert_eq!(typo.hunter_profile_names(&d.config), vec!["fast", "base"]);
        assert_eq!(u.npcs["deacon"], [4, 5]);
        assert_eq!(u.loot.oil, 10);
        assert!(u.requires.is_none());
        let c = d.zone("cistern").expect("cistern");
        assert_eq!(c.size, 64);
        let m = parse_zone(c).expect("parse");
        let water = m.cells.iter().filter(|&&k| k == CellKind::Water).count();
        assert_eq!(water, 1588);
        assert_eq!(m.creatures.len(), 2);
        assert_eq!(m.creatures[0].kind, CreatureKind::Drowner);
        assert_eq!(
            c.requires.as_ref().and_then(|r| r.building.clone()),
            Some("tram".into())
        );
        assert_eq!(c.anchor("hunters[1]"), Some([30, 40]));
        let o = d.zone("ossuary").expect("ossuary");
        assert_eq!(o.entry, EntryKind::Elevator);
        assert_eq!(o.burn_mul, 1.3);
        assert_eq!(o.creatures[0].facing, Some(Facing::N));
        assert!(o.creatures[0].gate_ok);
        let s = d.zone("source").expect("source");
        assert_eq!(s.deep_style, DeepStyle::Bands);
        assert_eq!(
            s.bands,
            Some(Bands {
                band: 5,
                max_lap: 5
            })
        );
        assert!(s.no_bank);
        assert_eq!(s.regions.len(), 21);
        assert_eq!(d.palettes["hub"].floor, 0x6e675e);
        assert_eq!(s.palette.fog.density, 0.12);
        assert_eq!(d.hub_rows.len(), 13);
        assert_eq!(d.hub_rows[0].len(), 25);
        let hub = d.parse_hub().expect("hub");
        assert_eq!(hub.anchors.len(), 7);
        assert_eq!(
            hub.stairs.map(|s| (s.marker.x, s.marker.z)),
            Some((70.5, 10.5))
        );
        assert_eq!(hub.flame.map(|f| (f.x, f.z)), Some((71.5, 1.5)));
    }

    #[test]
    fn tables_match_the_prototype() {
        let d = data();
        assert_eq!(d.contracts.contracts.len(), 8);
        assert_eq!(d.contracts.order.len(), 8);
        assert_eq!(d.contracts.order[0], "c_relight");
        let vigil = &d.contracts.contracts["c_vigil"];
        assert_eq!(vigil.kind, ContractKind::Survive);
        assert!(vigil.lamp_off);
        assert_eq!(vigil.seconds, Some(45.0));
        assert_eq!(vigil.reward.pts, 10);
        assert_eq!(
            d.contracts.contracts["c_censer"].item_kind,
            Some(ItemKind::Rich)
        );
        assert_eq!(
            d.contracts.contracts["c_relight"].reward.tool.as_deref(),
            Some("prybar")
        );
        assert_eq!(d.npcs.npcs.len(), 4);
        assert_eq!(d.npcs.npcs["deacon"].unlocks, "shrine");
        assert_eq!(d.npcs.looks["lamplighter"].hat, "stovepipe");
        assert_eq!(d.buildings.buildings.len(), 7);
        assert_eq!(
            d.buildings.order,
            vec!["board", "workshop", "press", "cart", "shrine", "tram", "elevator"]
        );
        assert!(d.buildings.buildings["board"].always);
        assert!((d.buildings.buildings["tram"].rot - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!(d.endgame.endings.len(), 3);
        assert_eq!(d.endgame.lap_lines.len(), 6);
        assert!(d.endgame.lap_lines[0].is_none());
        let dawn = d.endgame.ending("dawn").expect("dawn");
        assert_eq!(dawn.title, "The Lantern Eternal");
        assert_eq!(dawn.requires.map(|r| (r.tier, r.rescued)), Some((4, 3)));
        let cage = d.endgame.ending("cage").expect("cage");
        assert!(cage.lines[2]
            .render(1, &[])
            .starts_with("No one keeps the watch"));
        assert!(cage.lines[1]
            .render(4, &[])
            .starts_with("Far above, the Last Lantern blazes white"));
    }

    #[test]
    fn models_match_the_prototype() {
        let d = data();
        let hunter = d.models.get("hunter").expect("hunter");
        assert_eq!(hunter.box_count, 11);
        assert_eq!(hunter.boxes.len(), 11);
        let upper = hunter.part("upper").expect("upper pivot");
        assert_eq!(upper.pivot, [0.0, 0.6, 0.0]);
        assert!((upper.rotation[0] + 0.25).abs() < 1e-6);
        let eye = hunter.named("eyeL").expect("eyeL");
        assert_eq!(eye.emissive, Some(0xff3a20));
        assert_eq!(eye.part.as_deref(), Some("upper"));
        let torso = hunter.named("torso").expect("torso");
        assert_eq!(torso.color, 0x08080a);
        assert_eq!((torso.w, torso.h, torso.d), (0.5, 0.9, 0.35));
        assert_eq!(torso.y, 0.0);
        let warden = d.models.get("warden").expect("warden");
        assert_eq!(warden.part("head").map(|p| p.pivot), Some([0.0, 2.05, 0.0]));
        assert_eq!(
            warden.part("conePivot").and_then(|p| p.parent.clone()),
            Some("head".into())
        );
        assert_eq!(
            warden.part("legs1").map(|p| p.pivot),
            Some([0.18, 0.95, 0.0])
        );
        let lantern = d.models.get("lantern").expect("lantern");
        assert_eq!(
            lantern.named("glass").and_then(|b| b.emissive),
            Some(0xffc070)
        );
        assert_eq!(lantern.boxes.len(), 12);
        assert_eq!(
            d.models.get("hunterFast").map(|m| m.scale),
            Some([1.0, 1.15, 1.0])
        );
        assert_eq!(d.models.props.len(), 13);
        assert_eq!(d.models.props["rug"].len(), 7);
        assert!(d
            .models
            .get("drowner")
            .expect("drowner")
            .extras
            .contains(&"ripple".to_string()));
    }

    #[test]
    fn zone_locked_mirrors_js() {
        let d = data();
        let none = |_: &str| false;
        assert_eq!(d.zone_locked("undercroft", &none, 0, 1, &none), None);
        assert_eq!(
            d.zone_locked("ossuary", &none, 1, 1, &none),
            Some("Needs the Elevator and Light-tech II".into())
        );
        assert_eq!(
            d.zone_locked("source", &none, 0, 3, &none),
            Some("Needs flame tier 4 and Deacon Maud".into())
        );
        let all = |_: &str| true;
        assert_eq!(d.zone_locked("source", &all, 3, 4, &all), None);
        assert_eq!(
            d.zone_locked("nowhere", &all, 3, 4, &all),
            Some("Unknown zone".into())
        );
    }
}
