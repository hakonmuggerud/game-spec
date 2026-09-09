//! `hub.js:drawMinimap()` — the 200×200 map panel, redrawn at `HUB_CFG.minimapHz` (8 Hz) into an
//! RGBA8 [`Image`]. Everything here is a pure pixel writer over a byte buffer so it can be tested on a
//! 3×3 map with no renderer; the Bevy side only hands the buffer to `Assets<Image>`.
//!
//! Zone maps show explored cells only (the save's bitset, `save::bit_set`); the hub shows everything.

use undercroft_data::{CellKind, ItemKind, ParsedMap};

/// `hub.js:CELL_COLOR` — `#RRGGBB` per cell kind.
pub fn cell_color(t: CellKind) -> [u8; 4] {
    match t {
        CellKind::Floor | CellKind::Stairs | CellKind::Elevator => rgb(0x2a2a33),
        CellKind::Deep => rgb(0x16161f),
        CellKind::Water => rgb(0x1a2a3a),
        CellKind::Wall => rgb(0x555555),
        CellKind::Pillar => rgb(0x666666),
        CellKind::Gate => rgb(0x8a6a3a),
        CellKind::Altar => rgb(0x3a2a4a),
        CellKind::Shortcut => rgb(0x2f8f6a),
    }
}

/// `hub.js:SHORTCUT_OPEN_COLOR`.
pub const SHORTCUT_OPEN: [u8; 4] = rgb(0x5ff0b0);
/// `hub.js:ITEM_COLOR`.
pub fn item_color(kind: ItemKind) -> [u8; 4] {
    match kind {
        ItemKind::Oil => rgb(0xffa030),
        ItemKind::Relic => rgb(0x60e0ff),
        ItemKind::Rich => rgb(0xc070ff),
        ItemKind::Bundle => rgb(0xc8c8c8),
        ItemKind::Quest => rgb(0xe0d0a0),
    }
}

/// `▲ the way out`.
pub const STAIRS_COLOR: [u8; 4] = rgb(0x6a8aff);
/// The altar diamond.
pub const ALTAR_COLOR: [u8; 4] = rgb(0xc070ff);
/// The hub flame dot.
pub const FLAME_COLOR: [u8; 4] = rgb(0xffb265);
/// An active contract target.
pub const TARGET_COLOR: [u8; 4] = rgb(0xffd080);
/// A known but untargeted spot.
pub const SPOT_COLOR: [u8; 4] = rgb(0x8a7a5a);
/// A planted lantern.
pub const LANTERN_COLOR: [u8; 4] = rgb(0xffc070);
/// `hub.js:779` — a built hub building's anchor square.
pub const BUILDING_BUILT_COLOR: [u8; 4] = rgb(0xffd080);
/// `hub.js:779` — an unlocked-but-unbuilt (ghost) hub building's anchor square.
pub const BUILDING_GHOST_COLOR: [u8; 4] = rgb(0x5a5040);
/// `hub.js:795` — an NPC present in the zone (captive or following).
pub const NPC_COLOR: [u8; 4] = rgb(0xb8862a);
/// The player arrow.
pub const PLAYER_COLOR: [u8; 4] = [255, 255, 255, 255];

/// `#RRGGBB` → opaque RGBA.
const fn rgb(v: u32) -> [u8; 4] {
    [
        ((v >> 16) & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        (v & 0xff) as u8,
        255,
    ]
}

/// The canvas `drawMinimap` draws into: an RGBA8 buffer plus the cell→pixel transform it computes from
/// the map size (`px = clamp(floor(min(W/m.w, H/m.h)), 2, HUB_CFG.minimapPx)`, centred).
#[derive(Debug, Clone, PartialEq)]
pub struct MiniCanvas {
    pub w: u32,
    pub h: u32,
    /// Pixels per cell.
    pub px: i32,
    /// Left/top margin in pixels.
    pub ox: i32,
    pub oz: i32,
    /// `w * h * 4` bytes, RGBA, row-major.
    pub buf: Vec<u8>,
}

impl MiniCanvas {
    /// Size the canvas for a map (`hub.js:drawMinimap`'s first four lines).
    pub fn new(w: u32, h: u32, map: &ParsedMap, minimap_px: u32) -> MiniCanvas {
        let px = (w as i32 / map.w.max(1))
            .min(h as i32 / map.h.max(1))
            .min(minimap_px as i32)
            .max(2);
        MiniCanvas {
            w,
            h,
            px,
            ox: (w as i32 - map.w * px) / 2,
            oz: (h as i32 - map.h * px) / 2,
            buf: vec![0; (w * h * 4) as usize],
        }
    }

    /// `g.clearRect(0, 0, W, H)` — fully transparent (the panel's own background shows through).
    pub fn clear(&mut self) {
        self.buf.fill(0);
    }

    /// `X(wx)` — world x to canvas x.
    pub fn x_of(&self, wx: f32, map_ox: i32) -> f32 {
        self.ox as f32 + (wx - map_ox as f32) * self.px as f32
    }

    /// `Z(wz)`.
    pub fn z_of(&self, wz: f32) -> f32 {
        self.oz as f32 + wz * self.px as f32
    }

    /// One opaque pixel, ignoring anything outside the canvas.
    pub fn put(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        self.buf[i..i + 4].copy_from_slice(&c);
    }

    /// The pixel at `(x, y)`, for tests.
    pub fn get(&self, x: i32, y: i32) -> [u8; 4] {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return [0, 0, 0, 0];
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        [
            self.buf[i],
            self.buf[i + 1],
            self.buf[i + 2],
            self.buf[i + 3],
        ]
    }

    /// `g.fillRect`.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        for dy in 0..h {
            for dx in 0..w {
                self.put(x + dx, y + dy, c);
            }
        }
    }

    /// `dot(wx, wz, color, r)` — a filled circle in canvas space.
    pub fn dot(&mut self, cx: f32, cy: f32, r: f32, c: [u8; 4]) {
        let r = r.max(0.5);
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// `diamond(wx, wz, color, r)` — a 1 px outline (`|dx| + |dy| ≈ r`).
    pub fn diamond(&mut self, cx: f32, cy: f32, r: f32, c: [u8; 4]) {
        let r = r.max(1.5);
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = (x as f32 + 0.5 - cx).abs() + (y as f32 + 0.5 - cy).abs();
                if (d - r).abs() <= 0.75 {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// `tri(wx, wz, color, ang, r)` — the filled arrow marker, pointing along `ang` in the JS's
    /// screen convention (`+sin(ang), +cos(ang)`).
    pub fn tri(&mut self, cx: f32, cy: f32, r: f32, ang: f32, c: [u8; 4]) {
        let p = |a: f32| (cx + a.sin() * r, cy + a.cos() * r);
        let (ax, ay) = p(ang);
        let (bx, by) = p(ang + 2.5);
        let (dx, dy) = p(ang - 2.5);
        let x0 = ax.min(bx).min(dx).floor() as i32;
        let x1 = ax.max(bx).max(dx).ceil() as i32;
        let y0 = ay.min(by).min(dy).floor() as i32;
        let y1 = ay.max(by).max(dy).ceil() as i32;
        let edge = |px: f32, py: f32, qx: f32, qy: f32, rx: f32, ry: f32| {
            (qx - px) * (ry - py) - (qy - py) * (rx - px)
        };
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = edge(ax, ay, bx, by, px, py);
                let w1 = edge(bx, by, dx, dy, px, py);
                let w2 = edge(dx, dy, ax, ay, px, py);
                if (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0) {
                    self.put(x, y, c);
                }
            }
        }
    }
}

/// One world item on the map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MiniItem {
    pub kind: ItemKind,
    pub x: f32,
    pub z: f32,
}

/// A shortcut door: an opened one is drawn whether or not its cell was explored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MiniShortcut {
    pub cx: i32,
    pub cz: i32,
    pub x: f32,
    pub z: f32,
    pub open: bool,
}

/// `hub.js:779` — one `BUILD_ORDER` entry, at its anchor cell (hub only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MiniBuilding {
    pub x: f32,
    pub z: f32,
    pub built: bool,
}

/// `hub.js:795` — one NPC present in the zone: a captive (shown once its cell is explored) or the
/// current follower (always shown, explored or not).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MiniNpc {
    pub x: f32,
    pub z: f32,
    pub following: bool,
}

/// Everything `drawMinimap` overlays on the cells.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MiniMarks {
    pub items: Vec<MiniItem>,
    pub lanterns: Vec<(f32, f32)>,
    pub shortcuts: Vec<MiniShortcut>,
    /// Active contract targets in this zone.
    pub targets: Vec<(f32, f32)>,
    /// Hub buildings, in `BUILD_ORDER` (hub only).
    pub buildings: Vec<MiniBuilding>,
    /// NPCs present in the zone (zone only).
    pub npcs: Vec<MiniNpc>,
    /// `(x, z, yaw)` of the player, when standing on this map.
    pub player: Option<(f32, f32, f32)>,
}

/// `hub.js:drawMinimap()`. `bits` is the zone's explored bitset; `None` draws every cell (the hub).
pub fn draw(canvas: &mut MiniCanvas, map: &ParsedMap, bits: Option<&[u8]>, marks: &MiniMarks) {
    canvas.clear();
    let seen = |cx: i32, cz: i32| match bits {
        None => true,
        Some(b) => undercroft_sim::save::bit_set(b, map.idx(cx, cz)),
    };
    let px = canvas.px;
    for cz in 0..map.h {
        for cx in 0..map.w {
            if !seen(cx, cz) {
                continue;
            }
            let t = map.cells[map.idx(cx, cz)];
            canvas.fill_rect(
                canvas.ox + cx * px,
                canvas.oz + cz * px,
                px,
                px,
                cell_color(t),
            );
        }
    }
    let fpx = px as f32;
    let mox = map.ox;
    if let Some(st) = map.stairs {
        let (x, y) = (canvas.x_of(st.marker.x, mox), canvas.z_of(st.marker.z));
        canvas.tri(x, y, fpx * 0.7, std::f32::consts::PI, STAIRS_COLOR);
    }
    if let Some(a) = map.altar {
        if seen(a.cx, a.cz) {
            canvas.diamond(
                canvas.x_of(a.x, mox),
                canvas.z_of(a.z),
                fpx * 0.6,
                ALTAR_COLOR,
            );
        }
    }
    if let Some(f) = map.flame {
        canvas.dot(
            canvas.x_of(f.x, mox),
            canvas.z_of(f.z),
            fpx * 0.6,
            FLAME_COLOR,
        );
    }
    for b in &marks.buildings {
        let c = if b.built {
            BUILDING_BUILT_COLOR
        } else {
            BUILDING_GHOST_COLOR
        };
        let x0 = (canvas.x_of(b.x, mox) - fpx * 0.4).round() as i32;
        let y0 = (canvas.z_of(b.z) - fpx * 0.4).round() as i32;
        let sz = (fpx * 0.8).round() as i32;
        canvas.fill_rect(x0, y0, sz, sz, c);
    }
    for (x, z) in &marks.targets {
        canvas.diamond(
            canvas.x_of(*x, mox),
            canvas.z_of(*z),
            fpx * 0.6,
            TARGET_COLOR,
        );
    }
    for sp in &map.spots {
        let m = &sp.marker;
        if seen(m.cx, m.cz)
            && !marks
                .targets
                .iter()
                .any(|(x, z)| (x - m.x).abs() < 0.01 && (z - m.z).abs() < 0.01)
        {
            canvas.diamond(
                canvas.x_of(m.x, mox),
                canvas.z_of(m.z),
                fpx * 0.6,
                SPOT_COLOR,
            );
        }
    }
    for it in &marks.items {
        let (cx, cz) = cell_of(map, it.x, it.z);
        if seen(cx, cz) {
            canvas.dot(
                canvas.x_of(it.x, mox),
                canvas.z_of(it.z),
                fpx * 0.35,
                item_color(it.kind),
            );
        }
    }
    for (x, z) in &marks.lanterns {
        canvas.dot(
            canvas.x_of(*x, mox),
            canvas.z_of(*z),
            fpx * 0.45,
            LANTERN_COLOR,
        );
    }
    for s in &marks.shortcuts {
        if !s.open && !seen(s.cx, s.cz) {
            continue;
        }
        let c = if s.open {
            SHORTCUT_OPEN
        } else {
            cell_color(CellKind::Shortcut)
        };
        canvas.fill_rect(canvas.ox + s.cx * px, canvas.oz + s.cz * px, px, px, c);
        if s.open {
            canvas.diamond(
                canvas.x_of(s.x, mox),
                canvas.z_of(s.z),
                (fpx * 0.6).max(1.5),
                SHORTCUT_OPEN,
            );
        }
    }
    for n in &marks.npcs {
        let (cx, cz) = cell_of(map, n.x, n.z);
        if n.following || seen(cx, cz) {
            canvas.dot(
                canvas.x_of(n.x, mox),
                canvas.z_of(n.z),
                fpx * 0.4,
                NPC_COLOR,
            );
        }
    }
    if let Some((x, z, yaw)) = marks.player {
        canvas.tri(
            canvas.x_of(x, mox),
            canvas.z_of(z),
            fpx * 0.9,
            yaw + std::f32::consts::PI,
            PLAYER_COLOR,
        );
    }
}

/// `grid::to_cell` without the sim's bounds clamp — the minimap only asks whether a cell was seen.
fn cell_of(map: &ParsedMap, x: f32, z: f32) -> (i32, i32) {
    ((x - map.ox as f32).floor() as i32, z.floor() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::CellKind;

    /// A 3×3 map: walls around one floor cell, with the middle of the top row a stairs marker.
    fn tiny() -> ParsedMap {
        let mut m = ParsedMap {
            name: "tiny".into(),
            w: 3,
            h: 3,
            ox: 0,
            bands: None,
            cells: vec![CellKind::Wall; 9],
            items: vec![],
            stairs: None,
            hunter_spawns: vec![],
            npc_cells: vec![],
            spots: vec![],
            gates: vec![],
            shortcuts: vec![],
            flame: None,
            anchors: Default::default(),
            altar: None,
            creatures: vec![],
        };
        m.cells[4] = CellKind::Floor;
        m
    }

    #[test]
    fn the_canvas_centres_the_map_and_clamps_the_cell_size() {
        let m = tiny();
        let c = MiniCanvas::new(200, 200, &m, 5);
        assert_eq!(c.px, 5, "clamped to HUB_CFG.minimapPx");
        assert_eq!(c.ox, (200 - 3 * 5) / 2);
        assert_eq!(c.oz, (200 - 3 * 5) / 2);
        assert_eq!(c.buf.len(), 200 * 200 * 4);
        // a map wider than the canvas falls back to the 2 px floor
        let mut big = tiny();
        big.w = 300;
        big.h = 300;
        assert_eq!(MiniCanvas::new(200, 200, &big, 5).px, 2);
    }

    #[test]
    fn unexplored_cells_stay_transparent_and_explored_ones_take_their_colour() {
        let m = tiny();
        let mut c = MiniCanvas::new(30, 30, &m, 5);
        // nothing explored
        let bits = vec![0u8; 2];
        draw(&mut c, &m, Some(&bits), &MiniMarks::default());
        assert!(c.buf.iter().all(|b| *b == 0));
        // mark the middle cell (index 4)
        let mut bits = vec![0u8; 2];
        bits[0] = 1 << 4;
        draw(&mut c, &m, Some(&bits), &MiniMarks::default());
        let (x, y) = (c.ox + c.px, c.oz + c.px);
        assert_eq!(c.get(x, y), cell_color(CellKind::Floor));
        assert_eq!(
            c.get(x + c.px - 1, y + c.px - 1),
            cell_color(CellKind::Floor)
        );
        assert_eq!(
            c.get(x - 1, y),
            [0, 0, 0, 0],
            "the wall beside it is unseen"
        );
        // the hub draws everything
        draw(&mut c, &m, None, &MiniMarks::default());
        assert_eq!(c.get(x - 1, y), cell_color(CellKind::Wall));
    }

    #[test]
    fn items_lanterns_and_the_player_are_drawn_over_the_cells() {
        let m = tiny();
        let mut c = MiniCanvas::new(60, 60, &m, 5);
        let marks = MiniMarks {
            items: vec![MiniItem {
                kind: ItemKind::Relic,
                x: 1.5,
                z: 1.5,
            }],
            ..MiniMarks::default()
        };
        draw(&mut c, &m, None, &marks);
        let (x, y) = (c.x_of(1.5, 0), c.z_of(1.5));
        assert_eq!(c.get(x as i32, y as i32), item_color(ItemKind::Relic));
        // an unexplored item is hidden
        let bits = vec![0u8; 2];
        draw(&mut c, &m, Some(&bits), &marks);
        assert_eq!(c.get(x as i32, y as i32), [0, 0, 0, 0]);
        // the player arrow always draws
        let marks = MiniMarks {
            player: Some((1.5, 1.5, 0.0)),
            ..MiniMarks::default()
        };
        draw(&mut c, &m, Some(&bits), &marks);
        assert_eq!(c.get(x as i32, y as i32), PLAYER_COLOR);
    }

    #[test]
    fn an_open_shortcut_shows_through_the_fog() {
        let m = tiny();
        let mut c = MiniCanvas::new(60, 60, &m, 5);
        let bits = vec![0u8; 2];
        let marks = MiniMarks {
            shortcuts: vec![MiniShortcut {
                cx: 0,
                cz: 0,
                x: 0.5,
                z: 0.5,
                open: true,
            }],
            ..MiniMarks::default()
        };
        draw(&mut c, &m, Some(&bits), &marks);
        assert_eq!(c.get(c.ox, c.oz), SHORTCUT_OPEN);
        // barred and unseen: nothing
        let marks = MiniMarks {
            shortcuts: vec![MiniShortcut {
                cx: 0,
                cz: 0,
                x: 0.5,
                z: 0.5,
                open: false,
            }],
            ..MiniMarks::default()
        };
        draw(&mut c, &m, Some(&bits), &marks);
        assert_eq!(c.get(c.ox, c.oz), [0, 0, 0, 0]);
    }

    /// `hub.js:779` — one square per `BUILD_ORDER` entry at its anchor cell, coloured by build state.
    /// The hub always shows everything, so `bits` is `None`.
    #[test]
    fn hub_buildings_draw_their_anchor_square_built_or_ghost() {
        let m = tiny();
        let mut c = MiniCanvas::new(60, 60, &m, 5);
        let marks = MiniMarks {
            buildings: vec![
                MiniBuilding {
                    x: 0.5,
                    z: 0.5,
                    built: true,
                },
                MiniBuilding {
                    x: 2.5,
                    z: 2.5,
                    built: false,
                },
            ],
            ..MiniMarks::default()
        };
        draw(&mut c, &m, None, &marks);
        let (bx, by) = (c.x_of(0.5, 0) as i32, c.z_of(0.5) as i32);
        assert_eq!(c.get(bx, by), BUILDING_BUILT_COLOR);
        // a corner of the same cell, outside the 0.8-cell square, still shows the wall beneath it
        assert_eq!(c.get(c.ox, c.oz), cell_color(CellKind::Wall));
        let (gx, gy) = (c.x_of(2.5, 0) as i32, c.z_of(2.5) as i32);
        assert_eq!(c.get(gx, gy), BUILDING_GHOST_COLOR);
    }

    /// `hub.js:795` — a dot per NPC present in the zone: hidden in the fog unless it is the
    /// follower, which always shows.
    #[test]
    fn a_zone_npc_dot_shows_once_explored_or_always_while_following() {
        let m = tiny();
        let mut c = MiniCanvas::new(60, 60, &m, 5);
        let (x, y) = (c.x_of(1.5, 0) as i32, c.z_of(1.5) as i32);
        let marks = MiniMarks {
            npcs: vec![MiniNpc {
                x: 1.5,
                z: 1.5,
                following: false,
            }],
            ..MiniMarks::default()
        };
        // nothing explored: the captive is hidden
        let bits = vec![0u8; 2];
        draw(&mut c, &m, Some(&bits), &marks);
        assert_eq!(c.get(x, y), [0, 0, 0, 0]);
        // its cell explored: the captive shows
        let mut bits = vec![0u8; 2];
        bits[0] = 1 << 4;
        draw(&mut c, &m, Some(&bits), &marks);
        assert_eq!(c.get(x, y), NPC_COLOR);
        // a follower shows even in the fog
        let bits = vec![0u8; 2];
        let marks = MiniMarks {
            npcs: vec![MiniNpc {
                x: 1.5,
                z: 1.5,
                following: true,
            }],
            ..MiniMarks::default()
        };
        draw(&mut c, &m, Some(&bits), &marks);
        assert_eq!(c.get(x, y), NPC_COLOR);
    }
}
