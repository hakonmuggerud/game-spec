//! Cell kinds and the ASCII legend (`maps.js:T`, `maps.js:LEGEND`, DESIGN.md §3.1).

use serde::{Deserialize, Serialize};

/// Cell type stored in a parsed map (`maps.js:T`). The discriminants match the JS values so parity
/// fixtures (`cells` arrays) compare directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum CellKind {
    Floor = 0,
    Wall = 1,
    Pillar = 2,
    Deep = 3,
    Stairs = 4,
    Water = 5,
    Gate = 6,
    Elevator = 7,
    Altar = 8,
    Shortcut = 9,
}

impl CellKind {
    /// All kinds in discriminant order (`maps.js:T_NAME` order).
    pub const ALL: [CellKind; 10] = [
        CellKind::Floor,
        CellKind::Wall,
        CellKind::Pillar,
        CellKind::Deep,
        CellKind::Stairs,
        CellKind::Water,
        CellKind::Gate,
        CellKind::Elevator,
        CellKind::Altar,
        CellKind::Shortcut,
    ];

    /// `maps.js:T_NAME[t]`.
    pub fn name(self) -> &'static str {
        match self {
            CellKind::Floor => "floor",
            CellKind::Wall => "wall",
            CellKind::Pillar => "pillar",
            CellKind::Deep => "deep",
            CellKind::Stairs => "stairs",
            CellKind::Water => "water",
            CellKind::Gate => "gate",
            CellKind::Elevator => "elevator",
            CellKind::Altar => "altar",
            CellKind::Shortcut => "shortcut",
        }
    }

    /// From the JS numeric value (`maps.js:T`).
    pub fn from_u8(v: u8) -> Option<CellKind> {
        CellKind::ALL.get(v as usize).copied()
    }

    /// `maps.js:isSolid` on a bare kind: wall, pillar, closed gate or barred shortcut.
    pub fn is_solid(self) -> bool {
        matches!(
            self,
            CellKind::Wall | CellKind::Pillar | CellKind::Gate | CellKind::Shortcut
        )
    }

    /// `maps.js:isExtraction` — stairs or elevator.
    pub fn is_extraction(self) -> bool {
        matches!(self, CellKind::Stairs | CellKind::Elevator)
    }
}

/// What a legend character means to the parser (`maps.js:LEGEND`, DESIGN.md §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Legend {
    Wall,
    Pillar,
    Floor,
    Deep,
    Stairs,
    Elevator,
    /// `o`
    ItemOil,
    /// `r`
    ItemRelic,
    /// `R` — deep floor + rich relic.
    DeepItemRich,
    /// `H` — hunter spawn.
    Hunter,
    /// `F` — hub great flame.
    Flame,
    /// `N` — captive NPC.
    Npc,
    /// `C` — contract spot.
    Spot,
    Water,
    /// `X` — tool gate.
    Gate,
    /// `=` — shortcut door.
    Shortcut,
    Altar,
    /// `0`–`9` — hub building anchor.
    Anchor(u8),
    /// `L` `G` `Y` `B` — creature spawn on floor/deep.
    Creature(CreatureKind),
    /// `w` — water cell with a Drowner in it.
    WaterDrowner,
}

/// The five creature kinds (`maps.js:CREATURE_CHARS`, DESIGN.md §5). Serialised with the JS names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CreatureKind {
    #[serde(rename = "lampwight")]
    Lampwight,
    #[serde(rename = "warden")]
    Warden,
    #[serde(rename = "falseLight")]
    FalseLight,
    #[serde(rename = "drowner")]
    Drowner,
    #[serde(rename = "brute")]
    Brute,
}

impl CreatureKind {
    /// The JS profile / kind name (`'lampwight'`, `'falseLight'` …).
    pub fn js_name(self) -> &'static str {
        match self {
            CreatureKind::Lampwight => "lampwight",
            CreatureKind::Warden => "warden",
            CreatureKind::FalseLight => "falseLight",
            CreatureKind::Drowner => "drowner",
            CreatureKind::Brute => "brute",
        }
    }
}

/// `maps.js:LEGEND` — classify one legend character; `None` for a character outside the legend.
pub fn legend(ch: char) -> Option<Legend> {
    Some(match ch {
        '#' => Legend::Wall,
        'P' => Legend::Pillar,
        '.' => Legend::Floor,
        'D' => Legend::Deep,
        'S' => Legend::Stairs,
        'V' => Legend::Elevator,
        'o' => Legend::ItemOil,
        'r' => Legend::ItemRelic,
        'R' => Legend::DeepItemRich,
        'H' => Legend::Hunter,
        'F' => Legend::Flame,
        'N' => Legend::Npc,
        'C' => Legend::Spot,
        'W' => Legend::Water,
        'X' => Legend::Gate,
        '=' => Legend::Shortcut,
        'A' => Legend::Altar,
        'L' => Legend::Creature(CreatureKind::Lampwight),
        'G' => Legend::Creature(CreatureKind::Warden),
        'Y' => Legend::Creature(CreatureKind::FalseLight),
        'B' => Legend::Creature(CreatureKind::Brute),
        'w' => Legend::WaterDrowner,
        '0'..='9' => Legend::Anchor(ch as u8 - b'0'),
        _ => return None,
    })
}

/// `maps.js:LEGEND_CHARS` — is the character part of the legend?
pub fn is_legend_char(ch: char) -> bool {
    legend(ch).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legend_covers_every_char() {
        for ch in "#P.DSVorRHFNCWX=A0123456789LGYBw".chars() {
            assert!(is_legend_char(ch), "{ch}");
        }
        assert!(!is_legend_char('x'));
        assert_eq!(legend('7'), Some(Legend::Anchor(7)));
        assert_eq!(
            legend('Y'),
            Some(Legend::Creature(CreatureKind::FalseLight))
        );
    }

    #[test]
    fn kinds_match_js_values() {
        assert_eq!(CellKind::Shortcut as u8, 9);
        assert_eq!(CellKind::from_u8(5), Some(CellKind::Water));
        assert_eq!(CellKind::from_u8(10), None);
        assert!(CellKind::Gate.is_solid());
        assert!(!CellKind::Water.is_solid());
    }
}
