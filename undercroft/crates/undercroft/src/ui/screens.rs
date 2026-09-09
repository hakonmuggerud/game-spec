//! The `#menu` screens: everything `ui.js:showMenu` drew, with the content `hub.js` (departure board,
//! build, workshop, press, cartographer, shrine, ride), `npc.js` (dialogue), `endgame.js` (altar choice,
//! ending) and `ui.js:showDeath` built. All of it is pure: a [`TextScreen`] in, a rendered panel out, so
//! the layout is unit-testable without a window.
//!
//! `hub.js` keyed these off `[n]` prefixes in the line text; here a [`TextScreen`] carries its picks as
//! data — every one of them a [`DebugCommand`] the run or audio lane handles.

use undercroft_data::{GameData, ItemKind};
use undercroft_sim::contracts;
use undercroft_sim::economy::{self, EndingScreen, Screen};
use undercroft_sim::follower;
use undercroft_sim::save::SaveData;

use crate::debug::DebugCommand;

/// What a `[n]` line does when picked.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    /// Queue this command.
    Cmd(DebugCommand),
    /// Queue these commands in order (`select(z) → descend()` for the tram).
    Cmds(Vec<DebugCommand>),
}

/// One `ui.js:showMenu({title, lines, foot})` screen plus its key table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextScreen {
    pub title: String,
    /// Body lines; a leading `"  "` is dimmed, as `#menubody .dim` is.
    pub lines: Vec<String>,
    pub foot: String,
    /// Digit 1..=n, index 0 = `Digit1`. `None` means the digit is inert.
    pub picks: Vec<Option<Pick>>,
    /// Enter / Space / E (`hub.js:handleMenuKey`); `None` closes the menu.
    pub confirm: Option<Pick>,
    /// A pulsing `.cta` line under the body ("Click to continue").
    pub cta: String,
    /// `#death h1` is red.
    pub danger_title: bool,
}

/// `hub.js:haveLine()`.
fn have_line(save: &SaveData) -> String {
    format!(
        "  You have {} oil, {} relic{}, {} rich.",
        save.oil,
        save.relics,
        if save.relics == 1 { "" } else { "s" },
        save.rich
    )
}

/// `hub.js:openBoard()` — the zone list with lock reasons and the active contract targets.
pub fn board(data: &GameData, save: &SaveData, tier: u32) -> TextScreen {
    let sel = economy::selected(data, save).to_string();
    let targets = contracts::targets(data, save, None);
    let mut lines = Vec::new();
    let mut picks = Vec::new();
    for (i, z) in data.zones.iter().enumerate() {
        match economy::zone_locked(data, save, tier, &z.id) {
            Some(lock) => {
                lines.push(format!("  [{}] {} — locked: {lock}", i + 1, z.name));
            }
            None => {
                lines.push(format!(
                    "[{}] {}{}",
                    i + 1,
                    z.name,
                    if z.id == sel {
                        "   ◆ next descent"
                    } else {
                        ""
                    }
                ));
                if !z.threat.is_empty() {
                    lines.push(format!("      ⚠ {}", z.threat));
                }
            }
        }
        picks.push(Some(Pick::Cmd(DebugCommand::SelectZone(z.id.clone()))));
        for c in targets.iter().filter(|c| c.zone == z.id) {
            let spot = c.cell.and_then(|cell| {
                z.spots
                    .iter()
                    .find(|sp| sp.cell == cell)
                    .map(|sp| sp.label.as_str())
            });
            lines.push(format!(
                "      ◇ {}{}{}",
                c.title,
                spot.map(|s| format!(" — {s}")).unwrap_or_default(),
                if c.goal > 1 {
                    format!(" ({}/{})", c.progress.floor() as u32, c.goal)
                } else {
                    String::new()
                }
            ));
        }
    }
    if targets.is_empty() {
        lines.push(String::new());
        lines.push("  No contracts posted. The rescued will have work for you.".to_string());
    }
    TextScreen {
        title: "Departure Board".to_string(),
        lines,
        foot: "1–4 choose · Enter / E confirm · Esc close".to_string(),
        picks,
        confirm: None,
        ..TextScreen::default()
    }
}

/// `hub.js:openBuildMenu(id)` — a ghost building.
pub fn build_menu(data: &GameData, save: &SaveData, tier: u32, id: &str) -> Option<TextScreen> {
    let b = data.buildings.buildings.get(id)?;
    let st = economy::build_status(data, save, tier, id)?;
    let mut lines = vec![
        b.desc.clone(),
        String::new(),
        format!("Cost: {}", economy::cost_text(st.cost)),
        have_line(save),
        String::new(),
    ];
    if let Some(npc) = &b.npc {
        let name = data
            .npcs
            .npcs
            .get(npc)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| npc.clone());
        lines.push(format!("  {name} will keep it."));
    }
    let can = !st.built && st.unlocked && st.affordable;
    lines.push(match &st.reason {
        None => format!("[1] Build the {}", b.name),
        Some(why) => format!("  [1] Build the {} — {why}", b.name),
    });
    let pick = Pick::Cmd(DebugCommand::Build {
        id: id.to_string(),
        free: false,
    });
    Some(TextScreen {
        title: format!("{} (unbuilt)", b.name),
        lines,
        foot: "1 to build · Esc to close".to_string(),
        picks: vec![Some(pick.clone())],
        confirm: if can { Some(pick) } else { None },
        ..TextScreen::default()
    })
}

/// `hub.js:ROMAN`.
const ROMAN: [&str; 5] = ["none", "I", "II", "III", "IV"];

/// `hub.js:techLine(t, label)`.
fn tech_line(data: &GameData, t: usize, label: &str) -> String {
    match data.config.light_tech.get(t) {
        None => String::new(),
        Some(x) => format!(
            "{label}: lamp reach ×{}, burn ×{}, flash {} oil, lantern {} oil",
            x.dist_mul, x.burn_mul, x.flash_cost, x.lantern_cost
        ),
    }
}

/// `hub.js:costText` over a `LIGHT_TECH` relic cost.
fn relic_cost_text(c: &undercroft_data::config::RelicCost) -> String {
    economy::cost_text(undercroft_sim::events::BuildSpend {
        oil: 0,
        relics: c.relics,
        rich: c.rich,
    })
}

/// `hub.js:openWorkshop()`. `upgradeLightTech` has no [`DebugCommand`], so its row is inert.
pub fn workshop(data: &GameData, save: &SaveData) -> TextScreen {
    let cur = save.light_tech as usize;
    let next = cur + 1;
    let mut lines = vec![
        "\"Every lamp I ever lit is out. Let's fix that.\"".to_string(),
        String::new(),
    ];
    lines.push(if cur > 0 {
        tech_line(
            data,
            cur,
            &format!("Light-tech {} (yours)", ROMAN[cur.min(4)]),
        )
    } else {
        "Your handlamp is plain: reach ×1, burn ×1, flash 15 oil, lantern 20 oil.".to_string()
    });
    lines.push(String::new());
    let mut picks: Vec<Option<Pick>> = Vec::new();
    if next < data.config.light_tech.len() {
        let cost = &data.config.light_tech[next].cost;
        let ok = save.relics >= cost.relics && save.rich >= cost.rich;
        lines.push(format!(
            "{}[1] Light-tech {} — {}",
            if ok { "" } else { "  " },
            ROMAN[next.min(4)],
            relic_cost_text(cost)
        ));
        lines.push(format!("  {}", tech_line(data, next, "   gives")));
        picks.push(Some(Pick::Cmd(DebugCommand::UpgradeLightTech)));
    } else {
        lines.push("  Nothing more to learn here.".to_string());
    }
    lines.push(have_line(save));
    TextScreen {
        title: "Workshop".to_string(),
        lines,
        foot: "1 to upgrade · Esc to close".to_string(),
        picks,
        ..TextScreen::default()
    }
}

/// `hub.js:openPress()`.
pub fn press(data: &GameData, save: &SaveData) -> TextScreen {
    let bc = &data.config.build_costs;
    let lvl = save.reservoir as usize;
    let any = save.relics > 0;
    let d = |ok: bool| if ok { "" } else { "  " };
    let mut lines = vec![
        "\"Relics burn better than they pray.\"".to_string(),
        String::new(),
        format!("{}[1] Press one relic → {} oil", d(any), bc.press_relic_oil),
        format!(
            "{}[2] Press every relic ({} → {} oil)",
            d(any),
            save.relics,
            save.relics * bc.press_relic_oil
        ),
    ];
    let mut picks = vec![
        Some(Pick::Cmd(DebugCommand::PressRelics { n: Some(1) })),
        Some(Pick::Cmd(DebugCommand::PressRelics { n: None })),
    ];
    if lvl < bc.reservoir.len() {
        let need = bc.reservoir[lvl];
        lines.push(format!(
            "{}[3] Deeper reservoir ({lvl}/{}) — {need} relics: the lamp starts with +{} oil",
            d(save.relics >= need),
            bc.reservoir.len(),
            bc.reservoir_oil
        ));
        picks.push(Some(Pick::Cmd(DebugCommand::DeepenReservoir)));
    } else {
        lines.push(format!(
            "  Reservoir {lvl}/{}: the lamp starts with {} oil.",
            bc.reservoir.len(),
            economy::start_oil(
                &data.config,
                economy::tier_for(&data.config.tiers, save.points),
                save.reservoir
            )
        ));
    }
    lines.push(have_line(save));
    TextScreen {
        title: "Oil Press".to_string(),
        lines,
        foot: "1–3 choose · Esc to close".to_string(),
        picks,
        ..TextScreen::default()
    }
}

/// `hub.js:openCart()`.
pub fn cart(data: &GameData, save: &SaveData, pct: &[(String, u32)]) -> TextScreen {
    let _ = data;
    let _ = save;
    let mut lines = vec![
        "\"I mapped every one of these halls. Then they moved.\"".to_string(),
        String::new(),
        "Tab shows the map while you are below: explored halls, items you have seen, contract spots, the way out."
            .to_string(),
        String::new(),
    ];
    for (name, p) in pct {
        lines.push(format!("  {name} — {p}% charted"));
    }
    lines.push(String::new());
    lines.push("[1] Show or hide the map now".to_string());
    TextScreen {
        title: "Cartographer's Table".to_string(),
        lines,
        foot: "1 toggles · Esc to close".to_string(),
        picks: vec![Some(Pick::Cmd(DebugCommand::ToggleMinimap))],
        ..TextScreen::default()
    }
}

/// `hub.js:openShrine()`.
pub fn shrine(data: &GameData, save: &SaveData) -> TextScreen {
    let on = save.blessing;
    let lines = vec![
        "\"The Source can be fed, or freed. Both are prayers.\"".to_string(),
        String::new(),
        format!(
            "Blessing: {}. {} oil at each descent; when the dark takes you, half of each kind you carry is banked anyway.",
            if on { "LIT" } else { "unlit" },
            data.config.build_costs.blessing_oil
        ),
        String::new(),
        format!("[1] {}", if on { "Snuff the blessing" } else { "Light the blessing" }),
        have_line(save),
    ];
    TextScreen {
        title: "Shrine".to_string(),
        lines,
        foot: "1 toggles · Esc to close".to_string(),
        picks: vec![Some(Pick::Cmd(DebugCommand::ToggleBlessing))],
        ..TextScreen::default()
    }
}

/// `hub.js:openRide(id)` — the tram (Cistern) and the elevator (Ossuary, Source).
pub fn ride(data: &GameData, save: &SaveData, tier: u32, id: &str) -> Option<TextScreen> {
    let b = data.buildings.buildings.get(id)?;
    let zones: &[&str] = if id == "tram" {
        &["cistern"]
    } else {
        &["ossuary", "source"]
    };
    let mut lines = vec![b.desc.clone(), String::new()];
    let mut picks = Vec::new();
    for (i, z) in zones.iter().enumerate() {
        let name = economy::zone_name(data, z);
        match economy::zone_locked(data, save, tier, z) {
            Some(lock) => lines.push(format!("  [{}] {name} — locked: {lock}", i + 1)),
            None => lines.push(format!("[{}] Ride to {name}", i + 1)),
        }
        picks.push(Some(Pick::Cmds(vec![
            DebugCommand::SelectZone(z.to_string()),
            DebugCommand::CloseMenu,
            DebugCommand::Descend,
        ])));
    }
    Some(TextScreen {
        title: b.name.clone(),
        lines,
        foot: "1–2 ride · Esc to close".to_string(),
        picks,
        ..TextScreen::default()
    })
}

/// `npc.js:talk(id)` redrawn from the same inputs (`follower::dialogue`). Accepting is `player.rs`'s
/// (`Npcs::on_key` → `Accept(id)` + `CloseMenu`), so this screen carries no picks of its own.
pub fn dialog(data: &GameData, save: &SaveData, npc: &str) -> Option<TextScreen> {
    let def = data.npcs.npcs.get(npc)?;
    let offer = contracts::available(data, save, npc);
    let titles: Vec<String> = save
        .contracts
        .active
        .iter()
        .filter_map(|id| data.contracts.contracts.get(id))
        .filter(|c| c.poster == npc)
        .map(|c| c.title.clone())
        .collect();
    let dlg = follower::dialogue(def, offer.as_ref(), &titles);
    Some(TextScreen {
        title: dlg.title,
        lines: dlg.lines,
        foot: dlg.foot,
        ..TextScreen::default()
    })
}

/// `ui.js:showDeath(lostLoot, lostAny)`.
pub fn death(lost: &str, lost_any: bool) -> TextScreen {
    TextScreen {
        title: "THE DARK TOOK YOU".to_string(),
        lines: vec![
            format!("Lost: {lost}."),
            if lost_any {
                "Your bundle lies where you fell. The flame at the Lantern is untouched."
                    .to_string()
            } else {
                "You carried nothing down. The flame at the Lantern is untouched.".to_string()
            },
        ],
        foot: String::new(),
        // `reference/prototype/index.html:122` — verbatim; Enter/Space/E work too, as they do in the JS.
        cta: "Click to return to the Lantern".to_string(),
        // Keyboard confirm on `DEAD` is `player.rs::on_key`'s, not this field (that mode never
        // reaches `ui::keys`); it is set here so `ui::mouse`'s generic "click the cta" handler can
        // drive the click-anywhere-returns behaviour the same way it drives the ending screen's.
        confirm: Some(Pick::Cmd(DebugCommand::ReturnToHub)),
        danger_title: true,
        ..TextScreen::default()
    }
}

/// `endgame.js:renderChoice()` / `renderEnd(id)` — the altar overlay, whichever half is up.
pub fn ending(
    data: &GameData,
    save: &SaveData,
    tier: u32,
    scr: &EndingScreen,
) -> Option<TextScreen> {
    let info = economy::endgame_info(data, save, tier);
    match scr.screen? {
        Screen::Choice => {
            let mut lines = vec![
                "A bowl of stone, and in it a light that is not fire. It is older than the Lantern; the Lantern was lit from it."
                    .to_string(),
                "It can be fed. It can be carried. It can be put out.".to_string(),
                String::new(),
            ];
            let mut picks = Vec::new();
            for (n, e) in data.endgame.endings.iter().enumerate() {
                let id = &e.id;
                let seen = if save.endings.get(id) { "  (seen)" } else { "" };
                match economy::ending_available(e, &info) {
                    Ok(()) => {
                        lines.push(format!("[{}] {}{seen}", n + 1, e.choice));
                        picks.push(Some(Pick::Cmd(DebugCommand::Choose(id.clone()))));
                    }
                    Err(why) => {
                        lines.push(format!("  [{}] {}{seen}", n + 1, e.choice));
                        lines.push(format!("      {why}"));
                        // `endgame.js:choose` refuses it too (toast + uiError); the command still goes.
                        picks.push(Some(Pick::Cmd(DebugCommand::Choose(id.clone()))));
                    }
                }
            }
            lines.push(String::new());
            lines.push(format!(
                "  Flame tier {} · Rescued {}/{}",
                info.tier, info.rescued, info.total
            ));
            Some(TextScreen {
                title: "THE SOURCE".to_string(),
                lines,
                foot: "Press 1, 2 or 3 — Esc to step back".to_string(),
                picks,
                ..TextScreen::default()
            })
        }
        Screen::End => {
            let id = scr.current.clone()?;
            let e = data.endgame.ending(&id)?;
            let mut lines = economy::ending_lines(e, &info);
            lines.push(String::new());
            lines.push(format!("  {}", economy::stats_line(&info)));
            Some(TextScreen {
                title: e.title.to_uppercase(),
                lines,
                foot: String::new(),
                cta: "Click to continue".to_string(),
                confirm: Some(Pick::Cmd(DebugCommand::ContinueEnding)),
                ..TextScreen::default()
            })
        }
    }
}

/// `config.js:LABEL[kind]`.
pub fn item_label(data: &GameData, kind: ItemKind) -> &str {
    let l = &data.config.label;
    match kind {
        ItemKind::Oil => &l.oil,
        ItemKind::Relic => &l.relic,
        ItemKind::Rich => &l.rich,
        ItemKind::Bundle => &l.bundle,
        ItemKind::Quest => &l.quest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::GameData;

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    #[test]
    fn board_lists_every_zone_and_marks_the_selection() {
        let d = data();
        let mut save = SaveData::defaults(0.6);
        save.zone_selected = "undercroft".to_string();
        let s = board(&d, &save, 1);
        assert_eq!(s.picks.len(), d.zones.len());
        assert!(s.lines[0].contains("◆ next descent"));
        // a locked zone is dimmed and names its reason
        assert!(s
            .lines
            .iter()
            .any(|l| l.starts_with("  [") && l.contains("locked:")));
        assert_eq!(
            s.picks[1],
            Some(Pick::Cmd(DebugCommand::SelectZone(d.zones[1].id.clone())))
        );
    }

    #[test]
    fn build_menu_shows_the_cost_and_why_not() {
        let d = data();
        let save = SaveData::defaults(0.6);
        let s = build_menu(&d, &save, 1, "workshop").expect("workshop exists");
        assert!(s.title.ends_with("(unbuilt)"));
        assert!(s.lines.iter().any(|l| l.starts_with("Cost: ")));
        assert!(s.lines.last().unwrap().contains("[1] Build"));
        // nothing banked: the row is dim and Enter does not build
        assert!(s.lines.last().unwrap().starts_with("  ["));
        assert_eq!(s.confirm, None);
    }

    #[test]
    fn service_rows_carry_their_debug_commands() {
        let d = data();
        let save = SaveData::defaults(0.6);
        assert_eq!(
            workshop(&d, &save).picks[0],
            Some(Pick::Cmd(DebugCommand::UpgradeLightTech))
        );
        assert_eq!(
            press(&d, &save).picks[0],
            Some(Pick::Cmd(DebugCommand::PressRelics { n: Some(1) }))
        );
        assert_eq!(
            shrine(&d, &save).picks[0],
            Some(Pick::Cmd(DebugCommand::ToggleBlessing))
        );
        assert_eq!(
            cart(&d, &save, &[]).picks[0],
            Some(Pick::Cmd(DebugCommand::ToggleMinimap))
        );
    }

    #[test]
    fn death_screen_text_follows_the_bundle() {
        let s = death("2 flasks", true);
        assert_eq!(s.lines[0], "Lost: 2 flasks.");
        assert!(s.lines[1].starts_with("Your bundle lies"));
        assert!(death("nothing", false).lines[1].starts_with("You carried nothing"));
        // `reference/prototype/index.html:122` — the call to action is the prototype's, word for word.
        assert_eq!(s.cta, "Click to return to the Lantern");
        assert_eq!(s.confirm, Some(Pick::Cmd(DebugCommand::ReturnToHub)));
    }

    #[test]
    fn ending_choice_and_end_screens() {
        let d = data();
        let mut save = SaveData::defaults(0.6);
        let mut scr = EndingScreen {
            screen: Some(Screen::Choice),
            current: None,
        };
        let s = ending(&d, &save, 1, &scr).unwrap();
        assert_eq!(s.title, "THE SOURCE");
        assert_eq!(s.picks.len(), d.endgame.endings.len());
        // "Kindle a new flame" needs tier 4 + 3 rescued: dimmed with the reason under it
        assert!(s.lines.iter().any(|l| l.contains("Needs flame tier")));
        save.endings.set("cage", true);
        scr.screen = Some(Screen::End);
        scr.current = Some("cage".to_string());
        let e = ending(&d, &save, 1, &scr).unwrap();
        assert_eq!(e.title, "A BRIGHTER CAGE");
        assert_eq!(e.cta, "Click to continue");
        assert_eq!(e.confirm, Some(Pick::Cmd(DebugCommand::ContinueEnding)));
        assert!(e.lines.iter().any(|l| l.contains("Runs 0 · Deaths 0")));
    }

    #[test]
    fn dialog_rebuilds_the_npc_panel() {
        let d = data();
        let mut save = SaveData::defaults(0.6);
        save.rescued.set("lamplighter", true);
        let s = dialog(&d, &save, "lamplighter").unwrap();
        assert_eq!(s.title, d.npcs.npcs["lamplighter"].name);
        assert!(s.lines.last().unwrap().starts_with("[1] Accept contract"));
        assert_eq!(s.foot, "1 / Enter to accept · Esc to close");
    }
}
