//! The small data tables: contracts (`contracts.js:CONTRACTS`), NPCs (`npc.js:NPCS`, `models.js:NPC_LOOKS`),
//! hub buildings (`hub.js:BUILDINGS`) and the endgame data (`endgame.js:ENDINGS`, `LAP_LINES`).

use crate::config::{ContractCfg, EndgameCfg, HubCfg, NpcCfg};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Item kinds the player can carry (`ctx.player.carried` keys plus the death bundle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Oil,
    Relic,
    Rich,
    Quest,
    Bundle,
}

/// `contracts.js:CONTRACTS[id].type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContractKind {
    /// Lantern planted within `plant_r` of the spot.
    Plant,
    /// Bank ≥ `n` of `item_kind` from the zone in one run.
    Fetch,
    /// A `quest` item spawns at the spot; pick it up and bank it.
    Recover,
    /// `seconds` contiguous within `spot_r` of the spot (`lamp_off` variants need the lamp doused).
    Survive,
}

/// `contracts.js:CONTRACTS[id].reward` (`tool` / `pts` / `oil`; missing = 0 / None).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reward {
    pub tool: Option<String>,
    pub pts: u32,
    pub oil: u32,
}

/// One `contracts.js:CONTRACTS` entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractDef {
    pub id: String,
    pub poster: String,
    /// JS `type`.
    pub kind: ContractKind,
    pub zone: String,
    pub title: String,
    pub text: String,
    /// Contract spot index (`ZoneDef::spots[spot]`) for plant / recover / survive.
    pub spot: Option<u32>,
    /// Fetch: JS `kind` — the item kind to bank.
    pub item_kind: Option<ItemKind>,
    /// Fetch: how many.
    pub n: Option<u32>,
    /// Survive: contiguous seconds.
    pub seconds: Option<f32>,
    /// Survive: the lamp must be doused.
    pub lamp_off: bool,
    /// Recover: the quest item's display name.
    pub item: Option<String>,
    pub reward: Reward,
}

/// `contracts.js` — `CONTRACT_CFG`, `CONTRACT_ORDER` (posting order per NPC) and `CONTRACTS`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractTable {
    pub cfg: ContractCfg,
    pub order: Vec<String>,
    pub contracts: BTreeMap<String, ContractDef>,
}

/// One `npc.js:NPCS` entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpcDef {
    pub id: String,
    pub name: String,
    pub short: String,
    /// `"him"` / `"her"`.
    pub pronoun: String,
    /// Zone the captive is held in.
    pub zone: String,
    /// Fallback cell (`ZoneDef::npcs` is preferred).
    pub cell: [i32; 2],
    /// Building id this NPC unlocks.
    pub unlocks: String,
    /// Hub anchor digit the resident stands beside (+1 x).
    pub anchor: String,
    pub coat: u32,
    pub hat: u32,
    /// The one dialogue line.
    pub line: String,
}

/// `models.js:NPC_LOOKS[id]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpcLook {
    /// `stovepipe` / `flat` / `scarf` / `hood`.
    pub hat: String,
    pub hat_color: u32,
    pub coat: u32,
    pub lamp: bool,
    pub apron: Option<u32>,
}

/// `npc.js` — `NPC_CFG`, the NPC ids in table order and `NPCS`, plus `models.js:NPC_LOOKS`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpcTable {
    pub cfg: NpcCfg,
    pub order: Vec<String>,
    pub npcs: BTreeMap<String, NpcDef>,
    pub looks: BTreeMap<String, NpcLook>,
}

/// One `hub.js:BUILDINGS` entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildingDef {
    pub id: String,
    pub name: String,
    /// Hub anchor digit.
    pub anchor: String,
    /// NPC whose rescue unlocks it.
    pub npc: Option<String>,
    /// Flame tier that unlocks it.
    pub tier: Option<u32>,
    /// `models.js` factory name.
    pub model: String,
    /// Built from the start (the board).
    pub always: bool,
    /// Placement yaw (radians) and offsets from the anchor cell centre.
    pub rot: f32,
    pub dx: f32,
    pub dz: f32,
    pub desc: String,
}

/// `hub.js` — `HUB_CFG`, `BUILD_ORDER` and `BUILDINGS`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildingTable {
    pub cfg: HubCfg,
    pub order: Vec<String>,
    pub buildings: BTreeMap<String, BuildingDef>,
}

/// One line of an ending's text (`endgame.js:ENDINGS[id].lines(ctx)`), as a template. `{names}` is the rescued
/// NPC names joined `"A, B and C"` (`endgame.js:list`), `{count}` their number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndingLine {
    /// Always this text.
    Fixed(String),
    /// `met` when `tier >= min_tier`, else `unmet`.
    Tier {
        min_tier: u32,
        met: String,
        unmet: String,
    },
    /// `with` when anyone was rescued (or `without` is `None`), else `without`.
    Names {
        with: String,
        without: Option<String>,
    },
}

impl EndingLine {
    /// Render the line for a flame tier and a rescued-name list (mirrors `endgame.js:ENDINGS[id].lines`).
    pub fn render(&self, tier: u32, names: &[&str]) -> String {
        match self {
            EndingLine::Fixed(s) => s.clone(),
            EndingLine::Tier {
                min_tier,
                met,
                unmet,
            } => {
                if tier >= *min_tier {
                    met.clone()
                } else {
                    unmet.clone()
                }
            }
            EndingLine::Names { with, without } => {
                let t = match without {
                    Some(w) if names.is_empty() => w,
                    _ => with,
                };
                t.replace("{names}", &name_list(names))
                    .replace("{count}", &names.len().to_string())
            }
        }
    }
}

/// `endgame.js:list` — `"A"`, `"A and B"`, `"A, B and C"`.
pub fn name_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_string(),
        [head @ .., last] => format!("{} and {}", head.join(", "), last),
    }
}

/// `endgame.js:ENDINGS.dawn.available` — the only gated ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndingRequires {
    pub tier: u32,
    pub rescued: u32,
}

/// One `endgame.js:ENDINGS` entry. `available()` is data here: `requires == None` means always available;
/// `dawn` needs `tier >= requires.tier && rescued >= requires.rescued`, otherwise the reason is
/// `"Needs flame tier 4 (now T) and 3 rescued (now R)"` (each clause only when it fails, joined by `" and "`).
/// `lines()` is `EndingLine::render` per line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndingDef {
    pub id: String,
    pub title: String,
    /// The altar menu line.
    pub choice: String,
    /// The menu key (`Digit1`…).
    pub key: String,
    pub mark: String,
    pub requires: Option<EndingRequires>,
    pub lines: Vec<EndingLine>,
}

/// `endgame.js` — `ENDGAME` numbers, `LAP_LINES` (index = lap, `None` at 0) and `ENDINGS` in `ENDING_ORDER`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndgameData {
    pub cfg: EndgameCfg,
    pub lap_lines: Vec<Option<String>>,
    pub endings: Vec<EndingDef>,
}

impl EndgameData {
    /// Ending by id.
    pub fn ending(&self, id: &str) -> Option<&EndingDef> {
        self.endings.iter().find(|e| e.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_list_matches_js() {
        assert_eq!(name_list(&[]), "");
        assert_eq!(name_list(&["A"]), "A");
        assert_eq!(name_list(&["A", "B"]), "A and B");
        assert_eq!(name_list(&["A", "B", "C"]), "A, B and C");
    }

    #[test]
    fn render_variants() {
        let l = EndingLine::Names {
            with: "with {names} ({count})".into(),
            without: Some("alone".into()),
        };
        assert_eq!(l.render(1, &[]), "alone");
        assert_eq!(l.render(1, &["X", "Y"]), "with X and Y (2)");
        let l = EndingLine::Names {
            with: "{count} voices: {names}".into(),
            without: None,
        };
        assert_eq!(l.render(1, &[]), "0 voices: ");
    }
}
