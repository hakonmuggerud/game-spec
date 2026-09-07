//! Headless simulation for The Undercroft: plain structs and pure functions over `undercroft_data` types, no
//! engine dependency. Bevy owns entities and lifetimes and calls into this crate from `FixedUpdate`
//! ("functional core, ECS shell"). State changes return `Vec<SimEvent>` rather than mutating hidden globals.
//!
//! Module ownership (Phase 1). The contract step implemented [`grid`], [`player`], [`events`], [`rng`] and
//! [`fixtures`]; every other module is a lane-owned file whose doc comment names the lane and what it must hold:
//!
//! | Lane | Modules |
//! |---|---|
//! | world | [`pool`], [`collision`], [`validate`], [`world`] |
//! | creatures | [`creature`] (`creature/mod.rs` and its submodules) |
//! | economy | [`contracts`], [`follower`], [`economy`], [`save`] |
//!
//! Conventions (DESIGN.md §3): 1 cell = 1 world unit, cell `(cx, cz)` spans `x ∈ [cx, cx+1)`, `z ∈ [cz, cz+1)`,
//! `idx = cz * w + cx`, row 0 is north, the hub sits at `x + 60`, no Y movement.

pub mod collision;
pub mod contracts;
pub mod creature;
pub mod economy;
pub mod events;
pub mod fixtures;
pub mod follower;
pub mod grid;
pub mod player;
pub mod pool;
pub mod rng;
pub mod save;
pub mod validate;
pub mod world;

pub use events::SimEvent;
pub use grid::Field;
pub use player::PlayerView;
pub use pool::Pool;
pub use rng::SimRng;
pub use undercroft_data::DATA_VERSION;

#[cfg(test)]
mod tests {
    #[test]
    fn links_against_data() {
        assert_eq!(super::DATA_VERSION, 1);
    }
}
