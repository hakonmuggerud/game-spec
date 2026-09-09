//! `ui.js:makeListMenu` — one keyboard-driven list component with a stack of panels, used by the main
//! menu (`#title`) and the pause menu (`#pausemenu`). This module is the state machine only: it has no
//! Bevy types beyond `Resource`, so it is unit-testable headless; `ui::screens` builds the panels and
//! `ui::render` draws whatever [`MenuState::view`] returns.
//!
//! The JS panels held closures (`items()`, `run()`, `onEscape()`); a resource that tests inspect cannot,
//! so a panel is a [`PanelKind`] rebuilt from the live game state on every render (exactly what
//! `items()` did) and an item's `run` is the data enum [`ItemAct`].

use crate::debug::DebugCommand;

/// Which panel is on the stack. Its items are rebuilt from [`MenuCtx`] every time it is rendered, so
/// "Continue" appears with the save and the volume bar follows the setting (`ui.js:refreshMainMenu`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelKind {
    /// `ui.js:mainRoot()`.
    MainRoot,
    /// `ui.js:pauseRoot(inZone)`.
    PauseRoot,
    /// `ui.js:controlsPanel`.
    Controls,
    /// `ui.js:soundPanel`.
    Sound,
    /// `ui.js:confirmNewGame`.
    ConfirmNewGame,
    /// `ui.js:pauseRoot` "Return to main menu" while below.
    ConfirmLeaveRun,
    /// `ui.js:pauseRoot` "Clear save".
    ConfirmClearSave,
}

/// What activating an item does. `Push`/`Pop` are handled inside [`MenuState`]; everything else is
/// handed back to the caller, which turns it into a [`DebugCommand`] or an audio setting.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemAct {
    /// Open a sub-panel (`menu.push(...)`).
    Push(PanelKind),
    /// `backItem` — `menu.pop()`.
    Pop,
    /// Queue a command (`A.begin()`, `A.closePause()`, …).
    Cmd(DebugCommand),
    /// `soundPanel` volume: step by ±0.1 (`a.setVolume`). No `DebugCommand` carries this yet.
    Volume(i32),
    /// `soundPanel` mute (`a.toggleMute`).
    ToggleMute,
    /// A row that only reads (the "Nothing more to learn here." kind).
    Nothing,
}

/// One row (`item = { label, note?, disabled?, danger?, key?, run?, adjust? }`).
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// `it.key` — the shown shortcut; `None` shows the 1-based index, as the JS does.
    pub key: Option<String>,
    pub label: String,
    pub note: String,
    pub disabled: bool,
    pub danger: bool,
    /// `it.run`.
    pub act: ItemAct,
    /// `it.adjust(dir)` — ← / → and `[` / `]`.
    pub adjustable: bool,
}

impl Item {
    /// A plain row.
    pub fn new(label: impl Into<String>, act: ItemAct) -> Item {
        Item {
            key: None,
            label: label.into(),
            note: String::new(),
            disabled: false,
            danger: false,
            act,
            adjustable: false,
        }
    }

    /// `it.note` — the right-hand grey caption.
    pub fn note(mut self, note: impl Into<String>) -> Item {
        self.note = note.into();
        self
    }

    /// `it.key`.
    pub fn key(mut self, key: impl Into<String>) -> Item {
        self.key = Some(key.into());
        self
    }

    /// `it.danger`.
    pub fn danger(mut self) -> Item {
        self.danger = true;
        self
    }

    /// `it.adjust` present.
    pub fn adjustable(mut self) -> Item {
        self.adjustable = true;
        self
    }
}

/// A rendered panel: `{ title?, sub?, text?, table?, items(), foot?, onEscape?() }`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Panel {
    pub title: String,
    /// `p.sub` — the `#mmsub` line.
    pub sub: String,
    /// `p.text` — paragraphs above the list.
    pub text: Vec<String>,
    /// `p.table` — the Controls key table.
    pub table: Vec<(String, String)>,
    pub items: Vec<Item>,
    pub foot: String,
    /// `p.onEscape` at the root of the stack.
    pub escape: Option<ItemAct>,
    /// `panel.sel` — the row selected when the panel is pushed (the confirm panels open on "No").
    pub sel: usize,
    /// `h1` is the big title screen heading rather than the 18 px menu one.
    pub big_title: bool,
}

/// Everything a panel's `items()` closure read out of `ctx` (`ui.js` `A.saveInfo()`, `ctx.audio`,
/// `ctx.player.carried`). Built once per frame by the render system, so the panels stay pure.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MenuCtx {
    /// `A.saveInfo().hasProgress`.
    pub has_progress: bool,
    /// `A.saveInfo().text` — `save.js:summary().text`.
    pub save_text: String,
    /// `pauseRoot(inZone)`.
    pub in_zone: bool,
    /// `A.describe(carried)`.
    pub loot: String,
    /// `sum(carried) > 0`.
    pub has_loot: bool,
    /// `ctx.audio.vol`.
    pub volume: f32,
    /// `ctx.audio.muted`.
    pub muted: bool,
    /// `ui.js:VERSION` and the engine credit in the main menu's footer.
    pub version: String,
}

/// One frame of the open menu, for the renderer and for `mainMenuState()` / `pauseState()` in tests.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuView {
    pub panel: Panel,
    pub sel: usize,
    pub depth: usize,
}

/// `ui.js` `KEY_UP` / `KEY_DOWN` / `KEY_LEFT` / `KEY_RIGHT` / `KEY_GO`.
const KEY_UP: [&str; 2] = ["ArrowUp", "KeyW"];
const KEY_DOWN: [&str; 2] = ["ArrowDown", "KeyS"];
const KEY_LEFT: [&str; 2] = ["ArrowLeft", "BracketLeft"];
const KEY_RIGHT: [&str; 2] = ["ArrowRight", "BracketRight"];
const KEY_GO: [&str; 4] = ["Enter", "Space", "KeyE", "NumpadEnter"];

/// `/^(?:Digit|Numpad)([1-9])$/` — the 1-based row a digit picks.
fn digit(code: &str) -> Option<usize> {
    let rest = code
        .strip_prefix("Digit")
        .or_else(|| code.strip_prefix("Numpad"))?;
    match rest.parse::<usize>() {
        Ok(n) if (1..=9).contains(&n) => Some(n),
        _ => None,
    }
}

/// One stack frame: the panel and the row that was selected when a sub-panel was pushed over it.
#[derive(Debug, Clone, PartialEq)]
struct Frame {
    kind: PanelKind,
    sel: usize,
}

/// `makeListMenu(el)` — the stack, the selection and the key routing. `items` is a cache of the last
/// rendered rows, exactly as `m.items` was: [`MenuState::key`] needs it, so call [`MenuState::render`]
/// (or [`MenuState::view`], which renders) once per frame before feeding keys in.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MenuState {
    stack: Vec<Frame>,
    sel: usize,
    items: Vec<Item>,
}

/// What a key did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KeyResult {
    /// `m.key(code)` returned true.
    pub consumed: bool,
    /// The item's action, when activating one produced something the caller must perform.
    pub act: Option<ItemAct>,
}

impl MenuState {
    /// `m.isOpen()`.
    pub fn is_open(&self) -> bool {
        !self.stack.is_empty()
    }

    /// `m.depth()`.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// `m.sel`.
    pub fn sel(&self) -> usize {
        self.sel
    }

    /// `m.panel()` — the kind on top of the stack.
    pub fn kind(&self) -> Option<&PanelKind> {
        self.stack.last().map(|f| &f.kind)
    }

    /// The rows of the last [`MenuState::render`].
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// `m.open(panel)` — a fresh stack with this panel at its root.
    pub fn open(&mut self, kind: PanelKind, ctx: &MenuCtx) {
        let panel = build(&kind, ctx);
        self.stack = vec![Frame { kind, sel: 0 }];
        self.sel = panel.sel;
        self.render(ctx);
    }

    /// `ui.js:refreshMainMenu` — redraw the root list, keeping the selection when it is showing.
    pub fn reopen_root(&mut self, kind: PanelKind, ctx: &MenuCtx) {
        let keep = if self.depth() == 1 { self.sel } else { 0 };
        self.open(kind, ctx);
        self.sel = keep.min(self.items.len().saturating_sub(1));
        self.render(ctx);
    }

    /// `m.close()`.
    pub fn close(&mut self) {
        self.stack.clear();
        self.items.clear();
        self.sel = 0;
    }

    /// `m.push(panel)`.
    pub fn push(&mut self, kind: PanelKind, ctx: &MenuCtx) {
        if let Some(top) = self.stack.last_mut() {
            top.sel = self.sel;
        }
        let panel = build(&kind, ctx);
        self.sel = panel.sel;
        self.stack.push(Frame {
            kind,
            sel: panel.sel,
        });
        self.render(ctx);
    }

    /// `m.pop()` — false at the root.
    pub fn pop(&mut self, ctx: &MenuCtx) -> bool {
        if self.stack.len() <= 1 {
            return false;
        }
        self.stack.pop();
        self.sel = self.stack.last().map(|f| f.sel).unwrap_or(0);
        self.render(ctx);
        true
    }

    /// `m.render()` — rebuild `m.items` from the top panel and pull the selection onto an enabled row.
    pub fn render(&mut self, ctx: &MenuCtx) -> Option<Panel> {
        let kind = self.stack.last()?.kind.clone();
        let panel = build(&kind, ctx);
        self.items = panel.items.clone();
        if self.sel >= self.items.len() {
            self.sel = self.items.len().saturating_sub(1);
        }
        if self.items.get(self.sel).is_some_and(|it| it.disabled) {
            if let Some(i) = self.items.iter().position(|it| !it.disabled) {
                self.sel = i;
            }
        }
        Some(panel)
    }

    /// The frame's drawing input; `None` when the menu is closed.
    pub fn view(&mut self, ctx: &MenuCtx) -> Option<MenuView> {
        let panel = self.render(ctx)?;
        Some(MenuView {
            panel,
            sel: self.sel,
            depth: self.stack.len(),
        })
    }

    /// `m.move(dir)` — wraps, skipping disabled rows.
    pub fn move_sel(&mut self, dir: i32) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        let mut i = self.sel;
        for _ in 0..n {
            i = ((i as i32 + dir).rem_euclid(n as i32)) as usize;
            if !self.items[i].disabled {
                break;
            }
        }
        self.sel = i;
    }

    /// `m.activate(i)`.
    pub fn activate(&mut self, i: usize, ctx: &MenuCtx) -> Option<ItemAct> {
        let it = self.items.get(i)?.clone();
        if it.disabled {
            return None;
        }
        self.sel = i;
        match it.act {
            ItemAct::Push(kind) => {
                self.push(kind, ctx);
                None
            }
            ItemAct::Pop => {
                self.pop(ctx);
                None
            }
            // `m.activate` falls back to `adjust(+1)` when a row has no `run`; the volume row has both,
            // and `run` wraps 100% back to 0 rather than stepping (`ui.js:soundPanel`).
            other => {
                self.render(ctx);
                Some(other)
            }
        }
    }

    /// `m.adjust(dir)` — only rows with an `adjust` respond.
    pub fn adjust(&mut self, dir: i32, ctx: &MenuCtx) -> KeyResult {
        let Some(it) = self.items.get(self.sel) else {
            return KeyResult::default();
        };
        if !it.adjustable {
            return KeyResult::default();
        }
        let act = match it.act {
            ItemAct::Volume(_) => Some(ItemAct::Volume(dir)),
            _ => None,
        };
        self.render(ctx);
        KeyResult {
            consumed: act.is_some(),
            act,
        }
    }

    /// `m.key(code)` — the whole routing table in one place.
    pub fn key(&mut self, code: &str, ctx: &MenuCtx) -> KeyResult {
        if !self.is_open() {
            return KeyResult::default();
        }
        if KEY_UP.contains(&code) {
            self.move_sel(-1);
            return KeyResult {
                consumed: true,
                act: None,
            };
        }
        if KEY_DOWN.contains(&code) {
            self.move_sel(1);
            return KeyResult {
                consumed: true,
                act: None,
            };
        }
        if KEY_LEFT.contains(&code) {
            return self.adjust(-1, ctx);
        }
        if KEY_RIGHT.contains(&code) {
            return self.adjust(1, ctx);
        }
        if KEY_GO.contains(&code) {
            let act = self.activate(self.sel, ctx);
            return KeyResult {
                consumed: true,
                act,
            };
        }
        if let Some(n) = digit(code) {
            let act = if n <= self.items.len() {
                self.activate(n - 1, ctx)
            } else {
                None
            };
            return KeyResult {
                consumed: true,
                act,
            };
        }
        if code == "Escape" {
            if self.pop(ctx) {
                return KeyResult {
                    consumed: true,
                    act: None,
                };
            }
            if let Some(panel) = self.render(ctx) {
                if let Some(act) = panel.escape {
                    return KeyResult {
                        consumed: true,
                        act: if act == ItemAct::Nothing {
                            None
                        } else {
                            Some(act)
                        },
                    };
                }
            }
        }
        KeyResult::default()
    }
}

/// `ui.js` panel factories — one function per [`PanelKind`], rebuilt from [`MenuCtx`] on every render.
pub fn build(kind: &PanelKind, ctx: &MenuCtx) -> Panel {
    match kind {
        PanelKind::MainRoot => main_root(ctx),
        PanelKind::PauseRoot => pause_root(ctx),
        PanelKind::Controls => controls_panel(),
        PanelKind::Sound => sound_panel(ctx),
        PanelKind::ConfirmNewGame => confirm_new_game(),
        PanelKind::ConfirmLeaveRun => confirm_leave_run(ctx),
        PanelKind::ConfirmClearSave => confirm_clear_save(),
    }
}

/// `ui.js:mainRoot()`.
fn main_root(ctx: &MenuCtx) -> Panel {
    let mut items = Vec::new();
    if ctx.has_progress {
        items.push(Item::new("Continue", ItemAct::Cmd(DebugCommand::Begin)).note(&ctx.save_text));
    }
    // `newGame()` without `force` opens the confirm panel when there is a save to lose.
    items.push(
        Item::new(
            "New Game",
            if ctx.has_progress {
                ItemAct::Push(PanelKind::ConfirmNewGame)
            } else {
                ItemAct::Cmd(DebugCommand::NewGame)
            },
        )
        .note(if ctx.has_progress {
            "starts over"
        } else {
            "the Lantern is nearly out"
        }),
    );
    items.push(Item::new("Controls", ItemAct::Push(PanelKind::Controls)));
    items.push(Item::new("Sound", ItemAct::Push(PanelKind::Sound)));
    Panel {
        title: "THE UNDERCROFT".to_string(),
        big_title: true,
        text: vec![
            "The Last Lantern is nearly out. Below the vault lies a drowned ruin, black as pitch, \
             and something lives in it that is drawn to moving light. Carry your handlamp down, \
             bring back relics and oil, and feed the flame."
                .to_string(),
        ],
        items,
        foot: format!("The Undercroft — {} · ↑↓ Enter or click", ctx.version),
        escape: Some(ItemAct::Nothing),
        ..default_panel()
    }
}

/// `ui.js:pauseRoot(inZone)`.
fn pause_root(ctx: &MenuCtx) -> Panel {
    let items = vec![
        Item::new("Resume", ItemAct::Cmd(DebugCommand::ClosePause)),
        Item::new("Controls", ItemAct::Push(PanelKind::Controls)),
        Item::new("Sound", ItemAct::Push(PanelKind::Sound)),
        Item::new(
            "Return to main menu",
            if ctx.in_zone {
                ItemAct::Push(PanelKind::ConfirmLeaveRun)
            } else {
                ItemAct::Cmd(DebugCommand::OpenMainMenu)
            },
        )
        .note(if ctx.in_zone { "abandons this run" } else { "" }),
        Item::new("Clear save", ItemAct::Push(PanelKind::ConfirmClearSave))
            .note("wipes everything")
            .danger(),
    ];
    Panel {
        title: "PAUSED".to_string(),
        items,
        foot: "Esc resumes · ↑↓ Enter or click · 1–5".to_string(),
        escape: Some(ItemAct::Cmd(DebugCommand::ClosePause)),
        ..default_panel()
    }
}

/// `ui.js:CONTROLS`.
pub const CONTROLS: [(&str, &str); 16] = [
    ("Mouse", "Look (click to lock the pointer)"),
    (
        "Arrow keys",
        "Look without the mouse · move the menu selection",
    ),
    ("W A S D", "Move"),
    ("Shift", "Sprint — faster, but heard 9 u away"),
    ("F", "Handlamp on / off (dark = stealth, burns nothing)"),
    (
        "Q",
        "Flash — spend oil to stagger the hunter in front of you",
    ),
    (
        "R",
        "Plant lantern — spend oil for a pool of light it will not enter",
    ),
    (
        "E",
        "Interact: pick up · free · open gate · bank · talk · build · descend · confirm",
    ),
    ("T", "Pour a carried flask into the lamp (+25 oil)"),
    ("Tab", "Minimap (needs the Cartographer's Table)"),
    ("M", "Mute"),
    ("[  ]", "Volume down / up"),
    ("1 – 5", "Pick a line in any menu"),
    ("Enter / Space", "Confirm"),
    ("Esc", "Pause menu · close a menu · back"),
    ("Backspace ×2", "On the main menu: wipe the save (shortcut)"),
];

/// `ui.js:controlsPanel(menu)`.
fn controls_panel() -> Panel {
    Panel {
        title: "CONTROLS".to_string(),
        table: CONTROLS
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        items: vec![back_item()],
        foot: "Esc or Enter to go back".to_string(),
        ..default_panel()
    }
}

/// No `DebugCommand` writes `SaveRes.audio` yet (see the lane report), so the Sound panel's two
/// settings rows are drawn disabled. Flip this to `true` once `SetVolume` / `ToggleMute` exist.
pub const AUDIO_COMMANDS_EXIST: bool = false;

/// `ui.js:soundPanel(menu)`.
fn sound_panel(ctx: &MenuCtx) -> Panel {
    let n = (ctx.volume.clamp(0.0, 1.0) * 10.0).round() as usize;
    let bar: String = "█".repeat(n) + &"░".repeat(10 - n);
    let mut items = vec![
        Item::new(
            format!(
                "Volume  {bar}  {}%",
                (ctx.volume.clamp(0.0, 1.0) * 100.0).round()
            ),
            ItemAct::Volume(1),
        )
        .key("◂ ▸")
        .note("← → or [ ] to change")
        .adjustable(),
        Item::new(
            format!("Mute: {}", if ctx.muted { "ON" } else { "off" }),
            ItemAct::ToggleMute,
        )
        .note("M toggles in play"),
    ];
    if !AUDIO_COMMANDS_EXIST {
        for it in &mut items {
            it.disabled = true;
            it.note = "no command yet".to_string();
        }
    }
    items.push(back_item());
    Panel {
        title: "SOUND".to_string(),
        items,
        foot: "All sound is synthesised — nothing to download. Esc to go back".to_string(),
        ..default_panel()
    }
}

/// `ui.js:backItem(menu)`.
fn back_item() -> Item {
    Item::new("Back", ItemAct::Pop).key("←")
}

/// `ui.js:confirmPanel(...)` shared shape: two rows, "No" preselected.
fn confirm(title: &str, text: &str, yes: &str, no: &str, on_yes: DebugCommand) -> Panel {
    Panel {
        title: title.to_string(),
        text: vec![text.to_string()],
        items: vec![
            Item::new(yes, ItemAct::Cmd(on_yes)).danger(),
            Item::new(no, ItemAct::Pop),
        ],
        sel: 1,
        foot: "Esc to go back".to_string(),
        ..default_panel()
    }
}

/// `ui.js:confirmNewGame()`.
fn confirm_new_game() -> Panel {
    confirm(
        "NEW GAME",
        "This wipes your save: the flame, every building, everyone you rescued, every ending seen.",
        "Yes, start over",
        "No, keep my save",
        DebugCommand::NewGame,
    )
}

/// `ui.js:pauseRoot` — "Return to main menu" from a zone.
fn confirm_leave_run(ctx: &MenuCtx) -> Panel {
    let text = if ctx.has_loot {
        format!(
            "You are still below. Carried loot is LOST if you leave now: {}. Bank it at the stairs to keep it.",
            ctx.loot
        )
    } else {
        "You are still below. Anything you pick up before banking would be lost — right now you carry nothing."
            .to_string()
    };
    confirm(
        "LEAVE THE DARK?",
        &text,
        if ctx.has_loot {
            "Leave — lose the loot"
        } else {
            "Leave the run"
        },
        "Stay",
        DebugCommand::OpenMainMenu,
    )
}

/// `ui.js:pauseRoot` — "Clear save".
fn confirm_clear_save() -> Panel {
    confirm(
        "CLEAR SAVE?",
        "This wipes your save: the flame, every building, everyone you rescued, every ending seen. \
         You return to the main menu.",
        "Yes, wipe it",
        "No, keep it",
        DebugCommand::ClearSave,
    )
}

/// `Panel::default()` with the fields the factories always leave alone.
fn default_panel() -> Panel {
    Panel::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> MenuCtx {
        MenuCtx {
            has_progress: true,
            save_text: "Flame tier 2 · 1/4 rescued · 0/3 endings seen".to_string(),
            in_zone: false,
            loot: "nothing".to_string(),
            has_loot: false,
            volume: 0.6,
            muted: false,
            version: "prototype v2".to_string(),
        }
    }

    #[test]
    fn main_root_lists_continue_only_with_a_save() {
        let mut m = MenuState::default();
        let mut c = ctx();
        m.open(PanelKind::MainRoot, &c);
        let labels: Vec<&str> = m.items().iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Continue", "New Game", "Controls", "Sound"]);
        assert_eq!(m.items()[0].note, c.save_text);
        c.has_progress = false;
        m.open(PanelKind::MainRoot, &c);
        let labels: Vec<&str> = m.items().iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["New Game", "Controls", "Sound"]);
        // with no save "New Game" starts at once; with one it asks first
        assert_eq!(m.items()[0].act, ItemAct::Cmd(DebugCommand::NewGame));
    }

    #[test]
    fn open_close_depth_and_escape() {
        let mut m = MenuState::default();
        let c = ctx();
        assert!(!m.is_open());
        m.open(PanelKind::MainRoot, &c);
        assert!(m.is_open());
        assert_eq!(m.depth(), 1);
        // "Controls" pushes, Escape pops
        assert_eq!(m.key("Digit3", &c).act, None);
        assert_eq!(m.depth(), 2);
        assert_eq!(m.kind(), Some(&PanelKind::Controls));
        assert!(m.key("Escape", &c).consumed);
        assert_eq!(m.depth(), 1);
        // Escape at the main-menu root is consumed and does nothing
        let r = m.key("Escape", &c);
        assert!(r.consumed);
        assert_eq!(r.act, None);
        m.close();
        assert!(!m.is_open());
        assert!(!m.key("Enter", &c).consumed);
    }

    #[test]
    fn selection_wraps_and_skips_disabled_rows() {
        let mut m = MenuState::default();
        let c = ctx();
        m.open(PanelKind::MainRoot, &c);
        assert_eq!(m.sel(), 0);
        m.key("ArrowUp", &c);
        assert_eq!(m.sel(), 3, "wraps to the last row");
        m.key("KeyS", &c);
        assert_eq!(m.sel(), 0, "and back to the first");
        m.key("ArrowDown", &c);
        m.key("ArrowDown", &c);
        assert_eq!(m.sel(), 2);
        // a disabled row is skipped by move and refused by activate
        m.items[1].disabled = true;
        m.sel = 0;
        m.move_sel(1);
        assert_eq!(m.sel(), 2);
        assert_eq!(m.activate(1, &c), None);
    }

    #[test]
    fn digits_pick_rows_and_out_of_range_is_swallowed() {
        let mut m = MenuState::default();
        let c = ctx();
        m.open(PanelKind::MainRoot, &c);
        let r = m.key("Digit1", &c);
        assert_eq!(r.act, Some(ItemAct::Cmd(DebugCommand::Begin)));
        let r = m.key("Numpad9", &c);
        assert!(r.consumed);
        assert_eq!(r.act, None);
    }

    #[test]
    fn confirm_panels_open_on_no_and_the_root_selection_survives() {
        let mut m = MenuState::default();
        let c = ctx();
        m.open(PanelKind::MainRoot, &c);
        m.key("ArrowDown", &c); // New Game
        assert_eq!(m.sel(), 1);
        assert_eq!(m.key("Enter", &c).act, None);
        assert_eq!(m.kind(), Some(&PanelKind::ConfirmNewGame));
        assert_eq!(m.sel(), 1, "the confirm panel opens on \"No\"");
        assert_eq!(
            m.key("Digit1", &c).act,
            Some(ItemAct::Cmd(DebugCommand::NewGame))
        );
        m.key("Escape", &c);
        assert_eq!(m.depth(), 1);
        assert_eq!(m.sel(), 1, "back on \"New Game\"");
    }

    #[test]
    fn pause_root_escape_resumes_and_the_zone_variant_confirms() {
        let mut m = MenuState::default();
        let mut c = ctx();
        m.open(PanelKind::PauseRoot, &c);
        assert_eq!(
            m.key("Escape", &c).act,
            Some(ItemAct::Cmd(DebugCommand::ClosePause))
        );
        // in the hub "Return to main menu" leaves at once
        assert_eq!(
            m.activate(3, &c),
            Some(ItemAct::Cmd(DebugCommand::OpenMainMenu))
        );
        // below it asks, and names the loot
        c.in_zone = true;
        c.has_loot = true;
        c.loot = "2 flasks, 1 relic".to_string();
        m.open(PanelKind::PauseRoot, &c);
        assert_eq!(m.activate(3, &c), None);
        assert_eq!(m.kind(), Some(&PanelKind::ConfirmLeaveRun));
        let p = m.render(&c).unwrap();
        assert!(p.text[0].contains("2 flasks, 1 relic"));
        assert_eq!(p.items[0].label, "Leave — lose the loot");
    }

    #[test]
    fn sound_panel_bar_and_adjust() {
        let mut m = MenuState::default();
        let mut c = ctx();
        m.open(PanelKind::Sound, &c);
        assert_eq!(m.items()[0].label, "Volume  ██████░░░░  60%");
        c.muted = true;
        m.render(&c);
        assert_eq!(m.items()[1].label, "Mute: ON");
        assert_eq!(m.items()[2].label, "Back");
        if AUDIO_COMMANDS_EXIST {
            m.sel = 0;
            assert_eq!(m.key("ArrowRight", &c).act, Some(ItemAct::Volume(1)));
            assert_eq!(m.key("BracketLeft", &c).act, Some(ItemAct::Volume(-1)));
            m.sel = 1;
            assert_eq!(m.key("ArrowRight", &c), KeyResult::default());
            assert_eq!(m.key("Enter", &c).act, Some(ItemAct::ToggleMute));
        } else {
            // no DebugCommand writes SaveRes.audio yet: both rows are inert and the selection lands on Back
            assert!(m.items()[0].disabled && m.items()[1].disabled);
            assert_eq!(m.sel(), 2);
            assert_eq!(m.key("Enter", &c).act, None);
            assert_eq!(
                m.depth(),
                1,
                "\"Back\" pops, but Sound is already the root here"
            );
        }
    }

    #[test]
    fn reopen_root_keeps_the_selection() {
        let mut m = MenuState::default();
        let c = ctx();
        m.open(PanelKind::MainRoot, &c);
        m.key("ArrowDown", &c);
        m.reopen_root(PanelKind::MainRoot, &c);
        assert_eq!(m.sel(), 1);
        // from a sub-panel it returns to the root's first row
        m.key("Enter", &c);
        m.reopen_root(PanelKind::MainRoot, &c);
        assert_eq!(m.depth(), 1);
        assert_eq!(m.sel(), 0);
    }
}
