//! `PlayerView` — the read-only snapshot of the player that the senses, the follower and the contracts consume.
//!
//! Derived from what the JS reads off `ctx.player`: `hunter.js` (`genericSense`, `wardenSense`, `drownerSense`,
//! `falseLightSense`, `targetPos`, `pickWander`, `pickRest`: `x z lampOn flashT sprinting moving inWater inPool
//! lampLock`), `npc.js` (`x z follower`), `contracts.js` (`x z lampOn carried`), `main.js:updateLamp`
//! (`oil onDeep lap`) and `endgame.js` (`lap carried`). The ECS shell builds one per fixed tick from the player
//! entity; the sim never mutates it.

use undercroft_data::tables::ItemKind;

/// What the player carries (`ctx.player.carried`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Carried {
    pub oil: u32,
    pub relic: u32,
    pub rich: u32,
    pub quest: u32,
}

impl Carried {
    /// Count of one kind (`bundle` is never carried: 0).
    pub fn get(&self, kind: ItemKind) -> u32 {
        match kind {
            ItemKind::Oil => self.oil,
            ItemKind::Relic => self.relic,
            ItemKind::Rich => self.rich,
            ItemKind::Quest => self.quest,
            ItemKind::Bundle => 0,
        }
    }

    /// `endgame.js:carriedTotal`.
    pub fn total(&self) -> u32 {
        self.oil + self.relic + self.rich + self.quest
    }

    /// Nothing carried.
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// The NPC following the player, as the hunters sense it (`npc.js:stimulus()`): always "walking, lamp off",
/// `lit` when within `NPC_CFG.litR` of the lit player, `in_pool` when standing in a planted pool.
#[derive(Debug, Clone, PartialEq)]
pub struct FollowerView {
    pub id: String,
    pub x: f32,
    pub z: f32,
    pub moving: bool,
    pub lit: bool,
    pub in_pool: bool,
}

/// Read-only player snapshot for one simulation tick.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerView {
    /// World position (zone maps sit at `ox` 0, the hub at `HUB_OX`).
    pub x: f32,
    pub z: f32,
    /// Yaw in radians; forward = `(-sin yaw, -cos yaw)`.
    pub yaw: f32,
    /// The handlamp is lit (`ctx.player.lampOn`).
    pub lamp_on: bool,
    /// Seconds of flash burst left (`flashT`); `lit()` is true while it is positive.
    pub flash_t: f32,
    /// Lampwight relight lockout left (`lampLock`, seconds).
    pub lamp_lock: f32,
    pub sprinting: bool,
    /// Any movement input this frame (`moving`).
    pub moving: bool,
    /// Standing on a water cell (`inWater`).
    pub in_water: bool,
    /// Within `CFG.poolR` of a planted lantern (`inPool`).
    pub in_pool: bool,
    /// Standing on a deep cell (`onDeep`).
    pub on_deep: bool,
    /// Source lap of the cell (0 outside the Source).
    pub lap: i32,
    pub oil: f32,
    /// Effective lamp reach: `CFG.lampDist × zoneMul().dist` (`main.js:updateLamp` → `lamp.distance`).
    pub lamp_reach: f32,
    pub carried: Carried,
    /// The follower, when an NPC is following.
    pub follower: Option<FollowerView>,
}

impl PlayerView {
    /// `hunter.js:playerLit` — lamp on or mid-flash.
    pub fn lit(&self) -> bool {
        self.lamp_on || self.flash_t > 0.0
    }

    /// Standing still: no movement input.
    pub fn still(&self) -> bool {
        !self.moving
    }

    /// Wading: moving through water (what `genericSense` passes as the water stimulus).
    pub fn wading(&self) -> bool {
        self.in_water && self.moving
    }

    /// Follower position, if any.
    pub fn follower_pos(&self) -> Option<(f32, f32)> {
        self.follower.as_ref().map(|f| (f.x, f.z))
    }
}

impl Default for PlayerView {
    fn default() -> Self {
        PlayerView {
            x: 0.0,
            z: 0.0,
            yaw: 0.0,
            lamp_on: true,
            flash_t: 0.0,
            lamp_lock: 0.0,
            sprinting: false,
            moving: false,
            in_water: false,
            in_pool: false,
            on_deep: false,
            lap: 0,
            oil: 50.0,
            lamp_reach: 11.0,
            carried: Carried::default(),
            follower: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_and_wading() {
        let mut p = PlayerView {
            lamp_on: false,
            ..Default::default()
        };
        assert!(!p.lit());
        p.flash_t = 0.1;
        assert!(p.lit());
        p.in_water = true;
        assert!(!p.wading());
        p.moving = true;
        assert!(p.wading());
        assert!(!p.still());
        assert_eq!(Carried::default().get(ItemKind::Bundle), 0);
    }
}
