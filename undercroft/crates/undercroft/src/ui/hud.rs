//! `ui.js:updateHUD` + `hintText` and the HUD blocks the other modules appended to it: the contract
//! lines (`contracts.js:hudLines`), the follower line (`npc.js:updateHud`), the endings line
//! (`endgame.js`) and the hub's banked-resources block (`hub.js:updateHud`).
//!
//! The text assembly is pure (`oil_num`, `lamp_line`, `carried_line`, `flame_line`, `hint_line`) so it
//! is unit-tested without a window; the systems only move those strings into `Text` components.

use bevy::prelude::*;

use undercroft_data::config::Tier;
use undercroft_data::Config;
use undercroft_sim::creature::{self, Ctx as CreatureCtx, Env as CreatureEnv};
use undercroft_sim::economy::{self, LampState};
use undercroft_sim::player::Carried;
use undercroft_sim::{contracts, SimRng};

use crate::player::CurrentInteract;
use crate::resources::{Game, HubRes, LampRes, Npcs, Player, PlayerViewRes, SaveRes, ZoneRes};
use crate::state::{GameMode, Mode, PrevMode};
use crate::tick::Clock;
use crate::ui::style::*;
use crate::ui::UiState;

/* ============================================================
Pure text assembly (ui.js:updateHUD)
============================================================ */

/// `#oilnum` — `"50 / 100 oil"`.
pub fn oil_num(oil: f32, oil_max: f32) -> String {
    format!(
        "{} / {} oil",
        oil.clamp(0.0, oil_max).ceil() as i64,
        oil_max as i64
    )
}

/// `#lamp` — the hub always reads "safe" (`main.js:updateLamp` forces the lamp off there).
pub fn lamp_line(mode: GameMode, lamp_on: bool) -> String {
    if mode == GameMode::Hub {
        "LAMP OFF (safe)".to_string()
    } else if lamp_on {
        "LAMP ON".to_string()
    } else {
        "LAMP OFF".to_string()
    }
}

/// `#carried`.
pub fn carried_line(c: &Carried) -> String {
    if c.oil > 0 || c.relic > 0 || c.rich > 0 {
        format!(
            "Carried: {} flask · {} relic · {} rich",
            c.oil, c.relic, c.rich
        )
    } else {
        "Carried: nothing".to_string()
    }
}

/// `#flamehud` — `"Flame: tier 1 (0/6)"`, or `"… (12 pts)"` past the last tier.
pub fn flame_line(tiers: &[Tier], points: u32, tier: u32) -> String {
    let next = match tiers.get(tier as usize) {
        Some(t) => format!("{points}/{}", t.pts),
        None => format!("{points} pts"),
    };
    format!("Flame: tier {tier} ({next})")
}

/// `ui.js:hintText()` in full. `target_label` is `targetLabel(ctx.actions.interactTarget())` —
/// `player.rs` resolves it once per fixed tick into [`CurrentInteract`], both in the hub (where it
/// is the whole line) and in a zone (where it is one of the `·`-joined parts).
#[allow(clippy::too_many_arguments)]
pub fn hint_line(
    cfg: &Config,
    override_text: &str,
    mode: GameMode,
    seen: bool,
    creature_hint: &str,
    target_label: &str,
    lamp: &LampState,
    carried: &Carried,
    in_pool: bool,
) -> String {
    if !override_text.is_empty() {
        return override_text.to_string();
    }
    if mode == GameMode::Hub {
        return target_label.to_string();
    }
    if mode != GameMode::Zone {
        return String::new();
    }
    const NOT_SAFE: &str = "Not safe — it will wade in";
    let mut parts: Vec<String> = Vec::new();
    if seen {
        parts.push("It has seen you".to_string());
    }
    if !creature_hint.is_empty() && !parts.iter().any(|p| p == creature_hint) {
        parts.push(creature_hint.to_string());
    }
    // `ui.js:73` — the action prompt sits between the alarm lines and the lamp status.
    if !target_label.is_empty() {
        parts.push(target_label.to_string());
    }
    if lamp.oil <= 0.0 {
        parts.push(if carried.oil > 0 {
            "The lamp is dry — [T] pour a flask".to_string()
        } else {
            "The lamp is dry".to_string()
        });
    } else if lamp.lamp_on && lamp.oil < cfg.cfg.low_oil {
        parts.push("Lamp guttering".to_string());
    } else if in_pool && creature_hint != NOT_SAFE {
        parts.push("Safe — it will not enter the light".to_string());
    }
    parts.join("   ·   ")
}

/// `#vignette` opacity — darkens as the oil runs low, red while dying or dead.
pub fn vignette_alpha(cfg: &Config, mode: GameMode, oil: f32) -> f32 {
    if matches!(mode, GameMode::Dying | GameMode::Dead) {
        return 0.9;
    }
    if mode == GameMode::Zone && oil < cfg.cfg.low_oil {
        return 0.75 * (cfg.cfg.low_oil - oil) / cfg.cfg.low_oil;
    }
    0.0
}

/* ============================================================
Entities
============================================================ */

/// `#hud` — hidden on the title, death and ending screens (`ui.js:showTitle` / `showDeath`).
#[derive(Component)]
pub struct HudRoot;

/// `#oilfill`.
#[derive(Component)]
pub struct OilFill;

/// Which HUD text a `Text` entity is, so one system can fill them all.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudText {
    /// `#oilnum`.
    OilNum,
    /// `#lamp`.
    Lamp,
    /// `#cds` — the flash / lantern readiness pips, as text (`flash ● lantern ●`).
    Cooldowns,
    /// `#sound`.
    Sound,
    /// `#carried`.
    Carried,
    /// `#flamehud`.
    Flame,
    /// `#contracts`.
    Contracts,
    /// `#npcline`.
    Follower,
    /// `#endingshud`.
    Endings,
    /// `#hubres`.
    HubRes,
    /// `#hint`.
    Hint,
}

/// Spawn the HUD tree once (`index.html`'s `#hud` block).
pub fn spawn(commands: &mut Commands, font: &UiFont) {
    let root = commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            GlobalZIndex(2),
        ))
        .id();

    // #tl — the oil bar and the lamp line
    let tl = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(16.0),
                top: Val::Px(14.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    let bar_row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            ChildOf(tl),
        ))
        .id();
    let bar = commands
        .spawn((
            Node {
                width: Val::Px(200.0),
                height: Val::Px(12.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(DIM),
            BackgroundColor(OIL_BAR_BG),
            ChildOf(bar_row),
        ))
        .id();
    commands.spawn((
        OilFill,
        Node {
            width: Val::Percent(50.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(OIL),
        ChildOf(bar),
    ));
    text(commands, bar_row, font, HudText::OilNum, FS_HUD, FG);
    let lamp_row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(10.0),
                ..default()
            },
            ChildOf(tl),
        ))
        .id();
    text(commands, lamp_row, font, HudText::Lamp, FS_HUD, FG);
    text(commands, lamp_row, font, HudText::Cooldowns, FS_TINY, MUTED);
    text(commands, lamp_row, font, HudText::Sound, FS_HUD, OIL_LOW);

    // #tr — carried / flame / contracts / follower / endings
    let tr = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(16.0),
                top: Val::Px(14.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::End,
                row_gap: Val::Px(4.0),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    for (kind, size, colour) in [
        (HudText::Carried, FS_HUD, FG),
        (HudText::Flame, FS_HUD, FG),
        (HudText::Contracts, FS_SMALL, MUTED),
        (HudText::Follower, FS_SMALL, NPC_LINE),
        (HudText::Endings, FS_SMALL, ACCENT),
    ] {
        let e = text(commands, tr, font, kind, size, colour);
        commands
            .entity(e)
            .insert(TextLayout::justify(Justify::Right));
    }

    // #hubres — bottom left
    let e = text(commands, root, font, HudText::HubRes, FS_SMALL, MUTED);
    commands.entity(e).insert(Node {
        position_type: PositionType::Absolute,
        left: Val::Px(16.0),
        bottom: Val::Px(16.0),
        ..default()
    });

    // #hint — centred, 12 % up from the bottom
    let e = text(commands, root, font, HudText::Hint, FS_HINT, FG);
    commands.entity(e).insert((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            bottom: Val::Percent(12.0),
            width: Val::Percent(100.0),
            ..default()
        },
        TextLayout::justify(Justify::Center),
    ));

    // #dot — the crosshair
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            margin: UiRect {
                left: Val::Px(-2.0),
                top: Val::Px(-2.0),
                ..default()
            },
            width: Val::Px(4.0),
            height: Val::Px(4.0),
            ..default()
        },
        BackgroundColor(CROSSHAIR),
        ChildOf(root),
    ));
}

/// One HUD line.
fn text(
    commands: &mut Commands,
    parent: Entity,
    font: &UiFont,
    kind: HudText,
    size: f32,
    colour: Color,
) -> Entity {
    commands
        .spawn((
            kind,
            Text::new(""),
            font.at(size),
            TextColor(colour),
            ChildOf(parent),
        ))
        .id()
}

/* ============================================================
Systems
============================================================ */

/// Everything the HUD reads. Grouped to stay inside Bevy's system-parameter limit.
#[derive(bevy::ecs::system::SystemParam)]
pub struct HudSource<'w> {
    pub game: Game<'w>,
    pub save: Res<'w, SaveRes>,
    pub hub: Res<'w, HubRes>,
    pub lamp: Res<'w, LampRes>,
    pub player: Res<'w, Player>,
    pub view: Res<'w, PlayerViewRes>,
    pub zone: Res<'w, ZoneRes>,
    pub npcs: Res<'w, Npcs>,
    pub clock: Res<'w, Clock>,
    /// `main.js:53 state.prevMode` — `hub.js:updateHud` reads the mode *behind* an open menu.
    pub prev: Res<'w, PrevMode>,
    /// `ctx.actions.interactTarget()` as `player.rs` resolved it this tick, with its
    /// `ui.js:targetLabel` text.
    pub interact: Res<'w, CurrentInteract>,
}

/// `ui.js:update(c, dt)` → `updateHUD()`.
pub fn update_hud(
    src: HudSource,
    mode: Mode,
    ui: Res<UiState>,
    mut texts: Query<(&HudText, &mut Text)>,
    mut fill: Query<&mut Node, With<OilFill>>,
    mut fill_colour: Query<&mut BackgroundColor, With<OilFill>>,
    mut root: Query<&mut Node, (With<HudRoot>, Without<OilFill>)>,
) {
    if src.game.get().is_none() {
        return;
    }
    let cfg = src.game.config();
    let m = mode.get();
    let lamp = &src.lamp.0;
    let carried = &src.player.carried;
    // `hub.js:updateHud` — `const mode = st.mode === 'MENU' ? st.prevMode : st.mode`.
    let behind = match m {
        GameMode::Menu => src.prev.0.unwrap_or(GameMode::Hub),
        other => other,
    };
    let in_hub_block = behind == GameMode::Hub;

    // `ui.js:showTitle`/`showDeath`/`endgame.setHud` — the HUD is hidden on the full-screen panels.
    let want = if matches!(
        m,
        GameMode::Title | GameMode::Loading | GameMode::Dead | GameMode::Ending
    ) {
        Display::None
    } else {
        Display::Flex
    };
    if let Ok(mut n) = root.single_mut() {
        if n.display != want {
            n.display = want;
        }
    }
    if want == Display::None {
        return;
    }

    let oil = lamp.oil.clamp(0.0, cfg.cfg.oil_max);
    if let Ok(mut n) = fill.single_mut() {
        let pct = Val::Percent(100.0 * oil / cfg.cfg.oil_max);
        if n.width != pct {
            n.width = pct;
        }
    }
    if let Ok(mut c) = fill_colour.single_mut() {
        let want = if oil < cfg.cfg.low_oil { OIL_LOW } else { OIL };
        if c.0 != want {
            c.0 = want;
        }
    }

    // `ui.js:targetLabel(ctx.actions.interactTarget())` — resolved by `player.rs` (`track_interact`).
    let target_label = src.interact.label().to_string();
    let creature_hint = creature_hint(&src, m);
    let contracts_text = contracts::hud_lines(src.game.data(), &src.save.0).join("\n");
    let follower_text = src
        .npcs
        .0
        .hud_line(src.clock.time, matches!(m, GameMode::Zone | GameMode::Menu));
    let hub_res = if in_hub_block {
        economy::hub_hud_text(src.game.data(), &src.save.0)
    } else if behind == GameMode::Zone && src.hub.0.blessed {
        "Blessed".to_string()
    } else {
        String::new()
    };

    for (kind, mut t) in &mut texts {
        let s = match kind {
            HudText::OilNum => oil_num(oil, cfg.cfg.oil_max),
            HudText::Lamp => lamp_line(m, lamp.lamp_on),
            HudText::Cooldowns => format!(
                "flash {}  lantern {}",
                pip(lamp.flash_cd <= 0.0 && lamp.oil >= cfg.cfg.flash_cost),
                pip(lamp.lantern_cd <= 0.0 && lamp.oil >= cfg.cfg.lantern_cost),
            ),
            HudText::Sound => {
                if src.save.0.audio.muted {
                    "SOUND OFF".to_string()
                } else {
                    String::new()
                }
            }
            HudText::Carried => carried_line(carried),
            HudText::Flame => flame_line(&cfg.tiers, src.save.0.points, src.hub.0.tier),
            HudText::Contracts => contracts_text.clone(),
            HudText::Follower => follower_text.clone(),
            HudText::Endings => economy::endings_hud_line(&src.save.0, in_hub_block),
            HudText::HubRes => hub_res.clone(),
            HudText::Hint => hint_line(
                cfg,
                &ui.hint_override,
                m,
                ui.seen_t > 0.0,
                &creature_hint,
                &target_label,
                lamp,
                carried,
                src.player.in_pool,
            ),
        };
        if t.0 != s {
            t.0 = s;
        }
    }
}

/// `.cd` / `.cd.off` as text, since a `Node` pip cannot live inside a `Text`.
fn pip(ready: bool) -> &'static str {
    if ready {
        "●"
    } else {
        "○"
    }
}

/// `hunter.js:hint()` — the per-creature HUD row (`creature::hint`, a read-only sim call; the throwaway
/// RNG and event sink it needs are never touched by that function).
fn creature_hint(src: &HudSource, mode: GameMode) -> String {
    if mode != GameMode::Zone {
        return String::new();
    }
    let Some(zone) = src.zone.get() else {
        return String::new();
    };
    let lanterns: Vec<(f32, f32)> = zone.lanterns.iter().map(|l| (l.x, l.z)).collect();
    let items: Vec<(f32, f32)> = zone.items.iter().map(|i| (i.x, i.z)).collect();
    let env = CreatureEnv {
        map: &zone.map,
        pool: &zone.pool,
        player: &src.view.0,
        lanterns: &lanterns,
        items: &items,
        in_zone: true,
        time: src.clock.time,
    };
    let mut rng = SimRng::seed(0);
    let mut evs = Vec::new();
    let ctx = CreatureCtx::new(&env, src.game.tuning(), &mut rng, &mut evs);
    creature::hint(&ctx, &zone.hunters)
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::GameData;

    fn cfg() -> Config {
        GameData::from_dir(&GameData::workspace_data_dir())
            .expect("assets/data loads")
            .config
    }

    #[test]
    fn the_top_left_block_reads_like_the_prototype() {
        let c = cfg();
        assert_eq!(oil_num(49.2, 100.0), "50 / 100 oil");
        assert_eq!(oil_num(-3.0, 100.0), "0 / 100 oil");
        assert_eq!(lamp_line(GameMode::Hub, true), "LAMP OFF (safe)");
        assert_eq!(lamp_line(GameMode::Zone, true), "LAMP ON");
        assert_eq!(lamp_line(GameMode::Zone, false), "LAMP OFF");
        assert_eq!(vignette_alpha(&c, GameMode::Dead, 100.0), 0.9);
        assert_eq!(vignette_alpha(&c, GameMode::Zone, 100.0), 0.0);
        assert!(vignette_alpha(&c, GameMode::Zone, 0.0) > 0.7);
    }

    #[test]
    fn the_top_right_block_reads_like_the_prototype() {
        let c = cfg();
        assert_eq!(carried_line(&Carried::default()), "Carried: nothing");
        assert_eq!(
            carried_line(&Carried {
                oil: 2,
                relic: 1,
                rich: 0,
                quest: 0
            }),
            "Carried: 2 flask · 1 relic · 0 rich"
        );
        assert_eq!(
            flame_line(&c.tiers, 0, 1),
            format!("Flame: tier 1 (0/{})", c.tiers[1].pts)
        );
        assert_eq!(flame_line(&c.tiers, 12, 4), "Flame: tier 4 (12 pts)");
    }

    #[test]
    fn the_hint_line_joins_alarm_creature_and_lamp_state() {
        let c = cfg();
        let dry = LampState {
            oil: 0.0,
            ..LampState::default()
        };
        let low = LampState {
            oil: 5.0,
            lamp_on: true,
            ..LampState::default()
        };
        let ok = LampState {
            oil: 80.0,
            lamp_on: true,
            ..LampState::default()
        };
        let none = Carried::default();
        let flask = Carried {
            oil: 1,
            ..Carried::default()
        };
        // an override wins everywhere
        assert_eq!(
            hint_line(
                &c,
                "hold still",
                GameMode::Zone,
                true,
                "",
                "",
                &ok,
                &none,
                false
            ),
            "hold still"
        );
        // the hub shows the interact label alone
        assert_eq!(
            hint_line(
                &c,
                "",
                GameMode::Hub,
                false,
                "",
                "[E] Bank loot",
                &ok,
                &none,
                false
            ),
            "[E] Bank loot"
        );
        // nothing outside HUB / ZONE
        assert_eq!(
            hint_line(&c, "", GameMode::Dead, true, "x", "y", &ok, &none, false),
            ""
        );
        assert_eq!(
            hint_line(
                &c,
                "",
                GameMode::Zone,
                true,
                "It has seen you",
                "",
                &dry,
                &flask,
                false
            ),
            "It has seen you   ·   The lamp is dry — [T] pour a flask",
            "the creature line is not repeated when it equals the alarm"
        );
        assert_eq!(
            hint_line(&c, "", GameMode::Zone, false, "", "", &dry, &none, false),
            "The lamp is dry"
        );
        assert_eq!(
            hint_line(&c, "", GameMode::Zone, false, "", "", &low, &none, false),
            "Lamp guttering"
        );
        assert_eq!(
            hint_line(&c, "", GameMode::Zone, false, "", "", &ok, &none, true),
            "Safe — it will not enter the light"
        );
        // a Brute in reach replaces the Safe line
        assert_eq!(
            hint_line(
                &c,
                "",
                GameMode::Zone,
                false,
                "Not safe — it will wade in",
                "",
                &ok,
                &none,
                true
            ),
            "Not safe — it will wade in"
        );
    }
}
