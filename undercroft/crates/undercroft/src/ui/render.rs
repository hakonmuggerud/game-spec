//! Drawing a panel. Every full-screen overlay the lane shows — the main menu, the pause menu, the hub
//! menus, the death screen and the two ending screens — is one [`PanelView`], so `index.html`'s
//! `.screen > .box` markup exists exactly once here.
//!
//! The tree is rebuilt only when the view changes (`ui.js:setText`'s cache, one level up).

use bevy::prelude::*;

use crate::ui::menu::{MenuView, Panel};
use crate::ui::screens::TextScreen;
use crate::ui::style::*;

/// One row of a list menu (`.mi`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemView {
    pub key: String,
    pub label: String,
    pub note: String,
    pub disabled: bool,
    pub danger: bool,
}

/// Everything a `.screen` shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PanelView {
    pub title: String,
    /// `h1` at 26 px (the title screen) rather than `#menu h1`'s 18 px.
    pub big_title: bool,
    /// `#death h1` is red.
    pub danger_title: bool,
    /// `#mmsub`.
    pub sub: String,
    /// `.mtext` paragraphs.
    pub text: Vec<String>,
    /// `.mtable` rows.
    pub table: Vec<(String, String)>,
    /// `#menubody` lines; a `"  "` prefix dims one.
    pub lines: Vec<String>,
    /// `.mlist` rows.
    pub items: Vec<ItemView>,
    pub sel: usize,
    /// `.small` footer.
    pub foot: String,
    /// `.cta` — the pulsing "Click to continue".
    pub cta: String,
    /// `.screen` backdrop.
    pub backdrop: Color,
    /// `.box.mm { max-width: 620px }` vs `#menu .box { max-width: 580px }`. An explicit width (rather
    /// than min/max) so paragraph text is measured against a known line length on the first pass.
    pub width: f32,
}

impl PanelView {
    /// `ui.js:makeListMenu`'s render, as data (`#title` / `#pausemenu`).
    pub fn from_menu(v: &MenuView) -> PanelView {
        let Panel {
            title,
            sub,
            text,
            table,
            items,
            foot,
            big_title,
            ..
        } = &v.panel;
        PanelView {
            title: title.clone(),
            big_title: *big_title,
            danger_title: false,
            sub: sub.clone(),
            text: text.clone(),
            table: table.clone(),
            lines: Vec::new(),
            items: items
                .iter()
                .enumerate()
                .map(|(i, it)| ItemView {
                    key: it.key.clone().unwrap_or_else(|| (i + 1).to_string()),
                    label: it.label.clone(),
                    note: it.note.clone(),
                    disabled: it.disabled,
                    danger: it.danger,
                })
                .collect(),
            sel: v.sel,
            foot: foot.clone(),
            cta: String::new(),
            backdrop: MENU_BACKDROP,
            width: 620.0,
        }
    }

    /// `ui.js:showMenu({title, lines, foot})` and the screens built on it. A `[n]` line the screen
    /// left without a pick is dimmed like `hub.js`'s unaffordable rows, so an inert digit reads as
    /// inert.
    pub fn from_text(s: &TextScreen) -> PanelView {
        PanelView {
            title: s.title.clone(),
            big_title: s.danger_title,
            danger_title: s.danger_title,
            lines: s.lines.iter().map(|l| dim_if_inert(l, s)).collect(),
            foot: s.foot.clone(),
            cta: s.cta.clone(),
            backdrop: if s.danger_title || !s.cta.is_empty() {
                SCREEN_BACKDROP
            } else {
                MENU_BACKDROP
            },
            width: 580.0,
            ..PanelView::default()
        }
    }
}

/// The root of the currently drawn panel; despawned whole when the view changes.
#[derive(Component)]
pub struct ScreenRoot;

/// The list-menu row this entity draws (`d.dataset.i` in `ui.js`), so `ui::mouse` can map a mouse
/// `Interaction` back onto [`crate::ui::menu::MenuState::hover`] / `::activate` by index.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuRow(pub usize);

/// A hub-screen `[n]` line (1-based, matching [`crate::ui::screens::TextScreen::picks`] and the
/// digit keys) — the prototype's `[n]` lines were keyboard-only, but nothing stops a click from
/// reaching the same [`crate::ui::screens::Pick`].
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickLine(pub usize);

/// Build the `.screen > .box` tree for one view.
pub fn spawn_panel(commands: &mut Commands, font: &UiFont, v: &PanelView) {
    let root = commands
        .spawn((
            ScreenRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(v.backdrop),
            GlobalZIndex(5),
        ))
        .id();
    let boxx = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Px(v.width),
                padding: UiRect::axes(Val::Px(32.0), Val::Px(24.0)),
                border: UiRect::all(Val::Px(1.0)),
                row_gap: Val::Px(3.0),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(BORDER),
            BackgroundColor(PANEL_BG),
            ChildOf(root),
        ))
        .id();

    if !v.title.is_empty() {
        commands.spawn((
            Text::new(v.title.clone()),
            font.at(if v.big_title { FS_H1 } else { FS_H1_MENU }),
            TextColor(if v.danger_title { OIL_LOW } else { ACCENT }),
            Node {
                margin: UiRect::bottom(Val::Px(12.0)),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(boxx),
        ));
    }
    if !v.sub.is_empty() {
        line(commands, boxx, font, &v.sub, FS_TINY, DIM);
    }
    for p in &v.text {
        let e = line(commands, boxx, font, p, FS_BODY, MUTED);
        commands.entity(e).insert((Node {
            margin: UiRect::vertical(Val::Px(8.0)),
            flex_shrink: 0.0,
            ..default()
        },));
    }
    if !v.table.is_empty() {
        let t = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    margin: UiRect::vertical(Val::Px(10.0)),
                    row_gap: Val::Px(2.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                ChildOf(boxx),
            ))
            .id();
        for (k, val) in &v.table {
            let row = commands
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(14.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    ChildOf(t),
                ))
                .id();
            let e = line(commands, row, font, k, FS_SMALL, ACCENT);
            commands.entity(e).insert(Node {
                width: Val::Px(120.0),
                flex_shrink: 0.0,
                ..default()
            });
            line(commands, row, font, val, FS_SMALL, FG);
        }
    }
    for l in &v.lines {
        let dim = l.starts_with("  ");
        let e = line(commands, boxx, font, l, FS_BODY, if dim { DIM } else { FG });
        // A `[n]` line — dimmed or not — picks the same digit `ui::keys::menu_key` reads.
        if let Some(n) = pick_line_index(l) {
            commands
                .entity(e)
                .insert((PickLine(n), Interaction::default()));
        }
    }
    if !v.items.is_empty() {
        let list = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(10.0), Val::Px(4.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                ChildOf(boxx),
            ))
            .id();
        for (i, it) in v.items.iter().enumerate() {
            let sel = i == v.sel;
            let (kc, lc) = row_colours(it, sel);
            let row = commands
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        width: Val::Percent(100.0),
                        height: Val::Px(30.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BorderColor::all(if sel { BORDER } else { Color::NONE }),
                    BackgroundColor(if sel { SEL_BG } else { Color::NONE }),
                    ChildOf(list),
                    MenuRow(i),
                    Interaction::default(),
                ))
                .id();
            // One text node with two spans: separate `Text` nodes cannot be made to share a baseline
            // in `bevy_ui` 0.19, and the key/label pair has to line up.
            let text = commands
                .spawn((
                    Text::new(""),
                    font.at(FS_ITEM),
                    TextColor(lc),
                    Node {
                        align_self: AlignSelf::Center,
                        flex_shrink: 0.0,
                        ..default()
                    },
                    ChildOf(row),
                ))
                .id();
            commands.spawn((
                TextSpan::new(format!("{} {:<4}", if sel { "▸" } else { " " }, it.key)),
                font.at(FS_ITEM),
                TextColor(kc),
                ChildOf(text),
            ));
            commands.spawn((
                TextSpan::new(it.label.clone()),
                font.at(FS_ITEM),
                TextColor(lc),
                ChildOf(text),
            ));
            if !it.note.is_empty() {
                commands.spawn((
                    Text::new(it.note.clone()),
                    font.at(FS_TINY),
                    TextColor(DIM),
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(10.0),
                        top: Val::Px(9.0),
                        ..default()
                    },
                    ChildOf(row),
                ));
            }
        }
    }
    if !v.cta.is_empty() {
        let e = line(commands, boxx, font, &v.cta, FS_BODY, CTA);
        commands.entity(e).insert((
            Node {
                margin: UiRect::top(Val::Px(14.0)),
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                ..default()
            },
            TextLayout::justify(Justify::Center),
        ));
    }
    if !v.foot.is_empty() {
        let e = line(commands, boxx, font, &v.foot, FS_TINY, DIM);
        commands.entity(e).insert(Node {
            margin: UiRect::top(Val::Px(10.0)),
            flex_shrink: 0.0,
            ..default()
        });
    }
}

/// Prefix a `[n]` line with the dim marker when the screen left digit `n` without a pick.
fn dim_if_inert(line: &str, s: &TextScreen) -> String {
    let Some(rest) = line.strip_prefix('[') else {
        return line.to_string();
    };
    let Some((n, _)) = rest.split_once(']') else {
        return line.to_string();
    };
    let inert = n
        .parse::<usize>()
        .ok()
        .filter(|n| *n >= 1)
        .is_some_and(|n| !matches!(s.picks.get(n - 1), Some(Some(_))));
    if inert {
        format!("  {line}")
    } else {
        line.to_string()
    }
}

/// The 1-based digit a `[n]` line picks, regardless of a leading `"  "` dim prefix — mirrors
/// [`dim_if_inert`]'s own parse so a locked/unaffordable row stays clickable exactly where the
/// digit key reaches it.
fn pick_line_index(line: &str) -> Option<usize> {
    let rest = line.trim_start().strip_prefix('[')?;
    let (n, _) = rest.split_once(']')?;
    n.parse::<usize>().ok().filter(|n| *n >= 1)
}

/// `.mi` / `.mi.sel` / `.mi.dis` / `.mi.danger` colours.
fn row_colours(it: &ItemView, sel: bool) -> (Color, Color) {
    if it.disabled {
        return (DISABLED, DISABLED);
    }
    let label = match (it.danger, sel) {
        (true, true) => DANGER_SEL,
        (true, false) => DANGER,
        (false, true) => ACCENT,
        (false, false) => FG,
    };
    (if sel { ACCENT } else { DIM }, label)
}

/// One text node under `parent`.
fn line(
    commands: &mut Commands,
    parent: Entity,
    font: &UiFont,
    s: &str,
    size: f32,
    colour: Color,
) -> Entity {
    commands
        .spawn((
            Text::new(s.to_string()),
            font.at(size),
            TextColor(colour),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(parent),
        ))
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::screens::Pick;

    #[test]
    fn a_pick_with_no_command_is_drawn_dim() {
        let s = TextScreen {
            title: "Workshop".into(),
            lines: vec![
                "[1] Light-tech I — 6 relics".into(),
                "[2] Build the Workshop".into(),
                "plain line".into(),
            ],
            picks: vec![None, Some(Pick::Cmd(crate::debug::DebugCommand::Bank))],
            ..TextScreen::default()
        };
        let v = PanelView::from_text(&s);
        assert_eq!(v.lines[0], "  [1] Light-tech I — 6 relics");
        assert_eq!(v.lines[1], "[2] Build the Workshop");
        assert_eq!(v.lines[2], "plain line");
    }
}
