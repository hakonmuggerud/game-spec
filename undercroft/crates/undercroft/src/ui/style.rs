//! The prototype's palette and type scale (`prototype/index.html`'s stylesheet). Everything the lane
//! draws takes its colours and sizes from here, so a tweak lands in one place.

use bevy::prelude::*;
use bevy::text::FontSource;

/// `#RRGGBB` from the stylesheet as a Bevy colour.
pub const fn hex(v: u32) -> Color {
    Color::srgb(
        ((v >> 16) & 0xff) as f32 / 255.0,
        ((v >> 8) & 0xff) as f32 / 255.0,
        (v & 0xff) as f32 / 255.0,
    )
}

/// `#RRGGBB` plus an explicit alpha (the stylesheet's `#0009` / `#000d` forms).
pub const fn hexa(v: u32, a: f32) -> Color {
    Color::srgba(
        ((v >> 16) & 0xff) as f32 / 255.0,
        ((v >> 8) & 0xff) as f32 / 255.0,
        (v & 0xff) as f32 / 255.0,
        a,
    )
}

/// `body { color: #d8cfc0 }` — the default text colour.
pub const FG: Color = hex(0xd8cfc0);
/// `.small`, `.mn`, `#menubody .dim` — secondary text.
pub const DIM: Color = hex(0x8a7a5a);
/// `#contracts`, `#hubres`, `.tagline` — the muted HUD blocks.
pub const MUTED: Color = hex(0xa89878);
/// `h1`, `.mi.sel .ml`, `td:first-child` — the lamp-flame accent.
pub const ACCENT: Color = hex(0xffb265);
/// `.mi.dis .ml`.
pub const DISABLED: Color = hex(0x5a5048);
/// `.mi.danger .ml`.
pub const DANGER: Color = hex(0xd27a5a);
/// `.mi.danger.sel .ml`.
pub const DANGER_SEL: Color = hex(0xff8a5a);
/// `.screen .box` border.
pub const BORDER: Color = hex(0x5a4a30);
/// `.screen .box` background.
pub const PANEL_BG: Color = hex(0x0a0806);
/// `.mi.sel` background.
pub const SEL_BG: Color = hex(0x1a1510);
/// `#toast`.
pub const TOAST: Color = hex(0xffd59a);
/// `#oilfill`.
pub const OIL: Color = hex(0xe8a23a);
/// `#oilfill.low`, `#death h1`.
pub const OIL_LOW: Color = hex(0xd23a2a);
/// `#oilbar` background / `.cd.off`.
pub const OIL_BAR_BG: Color = hex(0x1a1510);
/// `.cd.off`.
pub const CD_OFF: Color = hex(0x3a2f22);
/// `npc.js:ensureHud` — the follower line.
pub const NPC_LINE: Color = hex(0xd8b070);
/// `.cta` — "Click to continue".
pub const CTA: Color = Color::WHITE;
/// `#dot` — the crosshair.
pub const CROSSHAIR: Color = Color::srgba(1.0, 1.0, 1.0, 0.6);

/// `.screen` backdrop (`#000d`).
pub const SCREEN_BACKDROP: Color = hexa(0x000000, 0.87);
/// `#menu` / `#pausemenu` backdrop (`#0009`).
pub const MENU_BACKDROP: Color = hexa(0x000000, 0.60);
/// `#minimap` background (`#000a`).
pub const MINIMAP_BG: Color = hexa(0x000000, 0.67);

/// `#tl` / `#tr` font size.
pub const FS_HUD: f32 = 13.0;
/// `#contracts`, `#hubres`, `#npcline` font size.
pub const FS_SMALL: f32 = 12.0;
/// `.small` / `#cds`.
pub const FS_TINY: f32 = 11.0;
/// `#hint`.
pub const FS_HINT: f32 = 15.0;
/// `#toast`.
pub const FS_TOAST: f32 = 14.0;
/// `.mi`.
pub const FS_ITEM: f32 = 14.0;
/// `p` / `#menubody div`.
pub const FS_BODY: f32 = 13.0;
/// `#menu h1`.
pub const FS_H1_MENU: f32 = 18.0;
/// `h1`.
pub const FS_H1: f32 = 26.0;

/// The lane's font: `assets/ui/AdwaitaMono-Regular.ttf` (SIL OFL 1.1) — the built-in `default_font`
/// subset has none of the box/geometric glyphs the sim's strings use (`◇ ◆ ✦ ✧ · — ⚠ █ ▸`).
pub const FONT_PATH: &str = "ui/AdwaitaMono-Regular.ttf";

/// The loaded font handle.
#[derive(Resource, Debug, Clone)]
pub struct UiFont(pub Handle<Font>);

impl UiFont {
    /// `font-size: <size>px` in the lane's font.
    pub fn at(&self, size: f32) -> TextFont {
        TextFont {
            font: FontSource::Handle(self.0.clone()),
            font_size: size.into(),
            ..default()
        }
    }
}
