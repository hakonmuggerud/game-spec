//! Pure grid helpers over a parsed map — the port of `maps.js` lines ~250–314 (`idx`, `inBounds`, `cellType`,
//! `isSolid`, `isBlocked`, `isExtraction`, `isWater`, `toCell`, `center`, `dist2d`, `bfsField`, `pathTo`,
//! `nearestReachable`, `los`), `maps.js:lapOf` and the route walk (`maps.js:passPred`, `routeCells`).
//!
//! Every function is pure. Coordinates follow DESIGN.md §3: cell `(cx, cz)` spans `x ∈ [ox+cx, ox+cx+1)`,
//! `z ∈ [cz, cz+1)`; `idx = cz * w + cx`; row 0 is north. World positions are `f32` at the API (Bevy's
//! transform precision) and the DDA runs in `f64` like the JS, so the results match the prototype bit for bit
//! on `f32`-representable inputs (verified against `assets/fixtures/*.json`).

use crate::pool::Pool;
use undercroft_data::zone::Bands;
use undercroft_data::{CellKind, ParsedMap};

/// The four neighbour steps in the JS order `[1,0] [-1,0] [0,1] [0,-1]` — the BFS visit order depends on it.
pub const DIRS4: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// `maps.js:idx` — `cz * w + cx`.
#[inline]
pub fn idx(m: &ParsedMap, cx: i32, cz: i32) -> usize {
    (cz * m.w + cx) as usize
}

/// `maps.js:inBounds`.
#[inline]
pub fn in_bounds(m: &ParsedMap, cx: i32, cz: i32) -> bool {
    cx >= 0 && cz >= 0 && cx < m.w && cz < m.h
}

/// `maps.js:cellType` — `Wall` outside the grid.
#[inline]
pub fn cell_type(m: &ParsedMap, cx: i32, cz: i32) -> CellKind {
    if in_bounds(m, cx, cz) {
        m.cells[idx(m, cx, cz)]
    } else {
        CellKind::Wall
    }
}

/// `maps.js:isSolid` — wall, pillar, closed gate or barred shortcut (opening a gate / shortcut rewrites the cell
/// to `Floor`, so this needs no extra state).
#[inline]
pub fn is_solid(m: &ParsedMap, cx: i32, cz: i32) -> bool {
    cell_type(m, cx, cz).is_solid()
}

/// `maps.js:isBlocked` — solid, or inside a planted lantern's pool.
#[inline]
pub fn is_blocked(m: &ParsedMap, pool: &Pool, cx: i32, cz: i32) -> bool {
    is_solid(m, cx, cz) || (in_bounds(m, cx, cz) && pool.is_pool(idx(m, cx, cz)))
}

/// `maps.js:isExtraction`.
#[inline]
pub fn is_extraction(t: CellKind) -> bool {
    t.is_extraction()
}

/// `maps.js:isWater`.
#[inline]
pub fn is_water(m: &ParsedMap, cx: i32, cz: i32) -> bool {
    cell_type(m, cx, cz) == CellKind::Water
}

/// `maps.js:toCell` — world position → cell (may be outside the grid; check with [`in_bounds`]).
#[inline]
pub fn to_cell(m: &ParsedMap, wx: f32, wz: f32) -> (i32, i32) {
    (
        (wx as f64 - m.ox as f64).floor() as i32,
        (wz as f64).floor() as i32,
    )
}

/// `maps.js:center` — the world centre of a cell.
#[inline]
pub fn center(m: &ParsedMap, cx: i32, cz: i32) -> (f32, f32) {
    ((m.ox + cx) as f32 + 0.5, cz as f32 + 0.5)
}

/// Cell centre from a cell index.
#[inline]
pub fn center_of(m: &ParsedMap, i: usize) -> (f32, f32) {
    let (cx, cz) = m.cell_of(i);
    center(m, cx, cz)
}

/// `maps.js:dist2d` — `Math.hypot`, narrowed to `f32`. Use [`dist2d_f64`] when the result is compared against
/// a threshold: the narrowing can round a distance a hair above the radius down onto it.
#[inline]
pub fn dist2d(ax: f32, az: f32, bx: f32, bz: f32) -> f32 {
    dist2d_f64(ax, az, bx, bz) as f32
}

/// `maps.js:dist2d` in the JS's own precision — `Math.hypot` over the `f64` promotions of the inputs, with no
/// narrowing. Threshold tests (`dist2d(...) <= r`) must use this so the boundary matches the prototype.
#[inline]
pub fn dist2d_f64(ax: f32, az: f32, bx: f32, bz: f32) -> f64 {
    (ax as f64 - bx as f64).hypot(az as f64 - bz as f64)
}

/// A BFS distance / parent field (`maps.js:bfsField`): `dist[i]` in cells, −1 unreachable; `parent[i]` the
/// index the cell was reached from, −1 for the start and unreachable cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub dist: Vec<i16>,
    pub parent: Vec<i32>,
}

impl Field {
    /// Distance to a cell, `None` when unreachable.
    pub fn dist_to(&self, i: usize) -> Option<i16> {
        match self.dist.get(i) {
            Some(&d) if d >= 0 => Some(d),
            _ => None,
        }
    }

    /// Is the cell reachable?
    pub fn reachable(&self, i: usize) -> bool {
        self.dist_to(i).is_some()
    }
}

/// `maps.js:bfsField(m, sx, sz, blocked)` — 4-neighbour BFS from a cell over every cell `blocked(cx, cz)` does
/// not reject. The start is never tested against `blocked`. Visit order is the JS order ([`DIRS4`]), so parents
/// and therefore paths are identical to the prototype's.
pub fn bfs_field(m: &ParsedMap, sx: i32, sz: i32, blocked: impl Fn(i32, i32) -> bool) -> Field {
    let n = (m.w * m.h) as usize;
    let mut dist = vec![-1i16; n];
    let mut parent = vec![-1i32; n];
    let mut q: Vec<u32> = Vec::with_capacity(n);
    let s = idx(m, sx, sz);
    dist[s] = 0;
    q.push(s as u32);
    let mut qh = 0;
    while qh < q.len() {
        let i = q[qh] as usize;
        qh += 1;
        let cx = i as i32 % m.w;
        let cz = i as i32 / m.w;
        for (dx, dz) in DIRS4 {
            let (nx, nz) = (cx + dx, cz + dz);
            if !in_bounds(m, nx, nz) {
                continue;
            }
            let j = idx(m, nx, nz);
            if dist[j] >= 0 || blocked(nx, nz) {
                continue;
            }
            dist[j] = dist[i] + 1;
            parent[j] = i as i32;
            q.push(j as u32);
        }
    }
    Field { dist, parent }
}

/// `maps.js:bfsField(m, sx, sz, true)` — blocked by solids only (pools ignored).
pub fn bfs_solid(m: &ParsedMap, sx: i32, sz: i32) -> Field {
    bfs_field(m, sx, sz, |cx, cz| is_solid(m, cx, cz))
}

/// `maps.js:bfsField(m, sx, sz)` — the default: blocked by solids and lantern pools.
pub fn bfs_blocked(m: &ParsedMap, pool: &Pool, sx: i32, sz: i32) -> Field {
    bfs_field(m, sx, sz, |cx, cz| is_blocked(m, pool, cx, cz))
}

/// `maps.js:pathTo` as cell indices: the chain of cells from the one after the start up to `ti` (empty when `ti`
/// is the start or unreachable).
pub fn path_cells(field: &Field, ti: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = ti as i32;
    while i >= 0 && field.parent[i as usize] >= 0 {
        out.push(i as usize);
        i = field.parent[i as usize];
    }
    out.reverse();
    out
}

/// `maps.js:pathTo(m, field, ti)` — the path to a cell as world cell centres (the start cell is not included).
pub fn path_to(m: &ParsedMap, field: &Field, ti: usize) -> Vec<(f32, f32)> {
    path_cells(field, ti)
        .into_iter()
        .map(|i| center_of(m, i))
        .collect()
}

/// `maps.js:nearestReachable(m, field, wx, wz)` — the reachable cell whose centre is nearest a world point
/// (first one wins ties, in index order); `None` when the field reaches nothing.
pub fn nearest_reachable(m: &ParsedMap, field: &Field, wx: f32, wz: f32) -> Option<usize> {
    let mut best = None;
    let mut bd = f64::INFINITY;
    for (i, &d) in field.dist.iter().enumerate() {
        if d < 0 {
            continue;
        }
        let (px, pz) = center_of(m, i);
        let dd = (px as f64 - wx as f64).hypot(pz as f64 - wz as f64);
        if dd < bd {
            bd = dd;
            best = Some(i);
        }
    }
    best
}

/// `maps.js:los(m, x0, z0, x1, z1)` — grid DDA line of sight (Amanatides–Woo) between two world points, blocked
/// by solid cells ([`is_solid`]: walls, pillars, closed gates, barred shortcuts); 200-step cap (a longer line is
/// "not seen"). The start cell itself counts: standing inside a solid cell sees nothing.
pub fn los(m: &ParsedMap, x0: f32, z0: f32, x1: f32, z1: f32) -> bool {
    los_f64(m, x0 as f64, z0 as f64, x1 as f64, z1 as f64)
}

/// [`los`] on `f64` inputs — the exact arithmetic of the JS (`f32` positions widen losslessly, so the two agree;
/// this one exists so parity tests can feed the fixtures' doubles unchanged).
pub fn los_f64(m: &ParsedMap, x0: f64, z0: f64, x1: f64, z1: f64) -> bool {
    let ox = m.ox as f64;
    let mut cx = (x0 - ox).floor() as i32;
    let mut cz = z0.floor() as i32;
    let ex = (x1 - ox).floor() as i32;
    let ez = z1.floor() as i32;
    let dx = x1 - x0;
    let dz = z1 - z0;
    let step_x = if dx > 0.0 { 1 } else { -1 };
    let step_z = if dz > 0.0 { 1 } else { -1 };
    let tdx = if dx != 0.0 {
        (1.0 / dx).abs()
    } else {
        f64::INFINITY
    };
    let tdz = if dz != 0.0 {
        (1.0 / dz).abs()
    } else {
        f64::INFINITY
    };
    let fx = x0 - ox - cx as f64;
    let fz = z0 - cz as f64;
    let mut tmx = if dx != 0.0 {
        (if dx > 0.0 { 1.0 - fx } else { fx }) * tdx
    } else {
        f64::INFINITY
    };
    let mut tmz = if dz != 0.0 {
        (if dz > 0.0 { 1.0 - fz } else { fz }) * tdz
    } else {
        f64::INFINITY
    };
    for _ in 0..200 {
        if is_solid(m, cx, cz) {
            return false;
        }
        if cx == ex && cz == ez {
            return true;
        }
        if tmx < tmz {
            cx += step_x;
            tmx += tdx;
        } else {
            cz += step_z;
            tmz += tdz;
        }
    }
    false
}

/// `maps.js:LEGACY_BANDS` — the 40×40 / band 3 / laps 0–5 spiral `lapOf` falls back to without a map.
pub const LEGACY_BANDS: Bands = Bands {
    band: 3,
    max_lap: 5,
};
/// `maps.js:LEGACY_BANDS.size`.
pub const LEGACY_SIZE: i32 = 40;

/// `maps.js:lapOf(cx, cz, m)` on an explicit grid: `ring = min(cx, cz, w−1−cx, h−1−cz)`,
/// `lap = min(max_lap, floor((ring − 1) / band))` (DESIGN.md §3.4). `bands == None` uses [`LEGACY_BANDS`].
/// The ring-0 border gives lap −1, exactly like the JS.
pub fn lap_of(cx: i32, cz: i32, w: i32, h: i32, bands: Option<Bands>) -> i32 {
    let b = bands.unwrap_or(LEGACY_BANDS);
    let band = if b.band != 0 {
        b.band
    } else {
        LEGACY_BANDS.band
    };
    let ring = cx.min(cz).min(w - 1 - cx).min(h - 1 - cz);
    b.max_lap.min((ring - 1).div_euclid(band))
}

/// `maps.js:lapOf(cx, cz, map)` — the map's own size and bands.
pub fn lap_of_map(m: &ParsedMap, cx: i32, cz: i32) -> i32 {
    lap_of(cx, cz, m.w, m.h, m.bands)
}

/// `maps.js:lapOf(cx, cz)` without a map — the legacy 40×40 spiral.
pub fn lap_of_legacy(cx: i32, cz: i32) -> i32 {
    lap_of(cx, cz, LEGACY_SIZE, LEGACY_SIZE, None)
}

/// `maps.js:passPred(gatesOpen, scOpen)` — the validator's blocked predicate: walls and pillars always block;
/// gates block unless `gates_open`; shortcuts block unless `sc_open`. Pools are ignored.
pub fn pass_pred(gates_open: bool, sc_open: bool) -> impl Fn(&ParsedMap, i32, i32) -> bool {
    move |m, cx, cz| {
        let t = cell_type(m, cx, cz);
        matches!(t, CellKind::Wall | CellKind::Pillar)
            || (t == CellKind::Gate && !gates_open)
            || (t == CellKind::Shortcut && !sc_open)
    }
}

/// `maps.js:routeCells` result: BFS cells walked and how many targets were unreachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Route {
    pub cells: i32,
    pub unreachable: i32,
}

/// `maps.js:routeCells(m, {gatesOpen, scOpen})` — the greedy nearest-first full-clear walk: entry → every item,
/// `N` and `C` (in that order for ties) → back to the entry (DESIGN.md §3.3 "route cells"). A map without a
/// spawn cell walks nothing.
pub fn route_cells(m: &ParsedMap, gates_open: bool, sc_open: bool) -> Route {
    let stairs = match &m.stairs {
        Some(s) => s.marker,
        None => return Route::default(),
    };
    let blocked = pass_pred(gates_open, sc_open);
    let mut left: Vec<(i32, i32)> = m
        .items
        .iter()
        .map(|i| (i.cx, i.cz))
        .chain(m.npc_cells.iter().map(|n| (n.cx, n.cz)))
        .chain(m.spots.iter().map(|s| (s.marker.cx, s.marker.cz)))
        .collect();
    let mut cur = (stairs.cx, stairs.cz);
    let mut cells = 0i32;
    let mut unreachable = 0i32;
    while !left.is_empty() {
        let f = bfs_field(m, cur.0, cur.1, |cx, cz| blocked(m, cx, cz));
        let mut best: Option<usize> = None;
        let mut bd = i32::MAX;
        for (i, t) in left.iter().enumerate() {
            let d = f.dist[idx(m, t.0, t.1)] as i32;
            if d >= 0 && d < bd {
                bd = d;
                best = Some(i);
            }
        }
        match best {
            None => {
                unreachable = left.len() as i32;
                break;
            }
            Some(i) => {
                cells += bd;
                cur = left.remove(i);
            }
        }
    }
    let f = bfs_field(m, cur.0, cur.1, |cx, cz| blocked(m, cx, cz));
    let back = f.dist[idx(m, stairs.cx, stairs.cz)] as i32;
    if back > 0 {
        cells += back;
    }
    Route { cells, unreachable }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{load_map_fixture, MapFixture, MAP_FIXTURES};
    use undercroft_data::GameData;

    fn data() -> GameData {
        GameData::from_dir(&GameData::workspace_data_dir()).expect("assets/data loads")
    }

    fn parsed(d: &GameData, f: &MapFixture) -> ParsedMap {
        match f.id.as_str() {
            "hub" => d.parse_hub().expect("hub parses"),
            "hub_v1" => {
                undercroft_data::parse_hub(f.rows.as_ref().expect("v1 rows"), d.config.hub_ox)
                    .expect("hub v1 parses")
            }
            id => d.parse_zone(id).expect("zone exists").expect("zone parses"),
        }
    }

    #[test]
    fn cells_and_markers_match_fixtures() {
        let d = data();
        for name in MAP_FIXTURES {
            let f = load_map_fixture(name);
            let m = parsed(&d, &f);
            assert_eq!((m.w, m.h, m.ox), (f.w, f.h, f.ox), "{name}: size");
            let cells: Vec<u8> = m.cells.iter().map(|&k| k as u8).collect();
            assert_eq!(cells, f.cells, "{name}: cells");
            let walkable = m
                .cells
                .iter()
                .enumerate()
                .filter(|(i, _)| !is_solid(&m, (*i as i32) % m.w, (*i as i32) / m.w))
                .count();
            assert_eq!(walkable, f.walkable, "{name}: walkable");
            for (kind, n) in &f.counts {
                let got = m.cells.iter().filter(|k| k.name() == kind).count();
                assert_eq!(got, *n, "{name}: count of {kind}");
            }
            assert_eq!(
                m.stairs.map(|s| [s.marker.cx, s.marker.cz]),
                Some(f.entry),
                "{name}: entry"
            );
            let mk = &f.markers;
            assert_eq!(m.items.len(), mk.items.len(), "{name}: items");
            for (a, b) in m.items.iter().zip(&mk.items) {
                assert_eq!((a.cx, a.cz), (b.cx, b.cz));
                assert_eq!(
                    serde_json::to_value(a.kind).expect("kind"),
                    serde_json::Value::String(b.kind.clone())
                );
            }
            let cells_of = |v: &[crate::fixtures::MarkerFixture]| -> Vec<(i32, i32, usize)> {
                v.iter().map(|x| (x.cx, x.cz, x.idx)).collect()
            };
            let mine = |v: &[undercroft_data::Marker]| -> Vec<(i32, i32, usize)> {
                v.iter().map(|x| (x.cx, x.cz, x.idx)).collect()
            };
            assert_eq!(
                mine(&m.hunter_spawns),
                cells_of(&mk.hunter_spawns),
                "{name}: hunters"
            );
            assert_eq!(mine(&m.npc_cells), cells_of(&mk.npc_cells), "{name}: npcs");
            let spots: Vec<_> = m.spots.iter().map(|s| s.marker).collect();
            assert_eq!(mine(&spots), cells_of(&mk.spots), "{name}: spots");
            let gates: Vec<_> = m.gates.iter().map(|g| g.marker).collect();
            assert_eq!(mine(&gates), cells_of(&mk.gates), "{name}: gates");
            let scs: Vec<_> = m.shortcuts.iter().map(|s| s.marker).collect();
            assert_eq!(mine(&scs), cells_of(&mk.shortcuts), "{name}: shortcuts");
            for (a, b) in m.shortcuts.iter().zip(&mk.shortcuts) {
                assert_eq!(a.id().map(String::from), b.id_str(), "{name}: shortcut id");
            }
            for (a, b) in m.gates.iter().zip(&mk.gates) {
                assert_eq!(a.id, b.id_str(), "{name}: gate id");
            }
            let creatures: Vec<_> = m.creatures.iter().map(|c| c.marker).collect();
            assert_eq!(
                mine(&creatures),
                cells_of(&mk.creatures),
                "{name}: creatures"
            );
            for (a, b) in m.creatures.iter().zip(&mk.creatures) {
                assert_eq!(
                    a.kind.js_name(),
                    b.kind.as_deref().unwrap_or(""),
                    "{name}: creature kind"
                );
            }
            assert_eq!(
                m.flame.map(|x| (x.cx, x.cz)),
                mk.flame.as_ref().map(|x| (x.cx, x.cz)),
                "{name}: flame"
            );
            assert_eq!(
                m.altar.map(|x| (x.cx, x.cz)),
                mk.altar.as_ref().map(|x| (x.cx, x.cz)),
                "{name}: altar"
            );
            assert_eq!(m.anchors.len(), mk.anchors.len(), "{name}: anchors");
            for (k, v) in &mk.anchors {
                let d: u8 = k.parse().expect("digit");
                assert_eq!(
                    m.anchors.get(&d).map(|a| (a.cx, a.cz, a.x, a.z)),
                    Some((v.cx, v.cz, v.x, v.z))
                );
            }
        }
    }

    #[test]
    fn bfs_field_matches_fixtures() {
        let d = data();
        for name in MAP_FIXTURES {
            let f = load_map_fixture(name);
            let m = parsed(&d, &f);
            let field = bfs_solid(&m, f.entry[0], f.entry[1]);
            assert_eq!(field.dist, f.bfs_dist, "{name}: dist");
            assert_eq!(field.parent, f.bfs_parent, "{name}: parent");
            // the pool-aware default with an empty pool is the same field
            let pool = Pool::empty(m.len());
            assert_eq!(
                bfs_blocked(&m, &pool, f.entry[0], f.entry[1]),
                field,
                "{name}: pool field"
            );
        }
    }

    #[test]
    fn paths_match_fixtures() {
        let d = data();
        for name in MAP_FIXTURES {
            let f = load_map_fixture(name);
            let m = parsed(&d, &f);
            let entry = bfs_solid(&m, f.entry[0], f.entry[1]);
            for p in &f.paths {
                let field = if p.from == "entry" {
                    entry.clone()
                } else {
                    let (_, c) = f.anchor(&p.from);
                    bfs_solid(&m, c[0], c[1])
                };
                let ti = idx(&m, p.target[0], p.target[1]);
                let got: Vec<[i32; 2]> = path_to(&m, &field, ti)
                    .into_iter()
                    .map(|(x, z)| {
                        let (cx, cz) = to_cell(&m, x, z);
                        [cx, cz]
                    })
                    .collect();
                assert_eq!(got, p.cells, "{name}: path {} -> {}", p.from, p.to);
            }
        }
    }

    #[test]
    fn los_matches_fixtures() {
        let d = data();
        let mut n = 0;
        for name in MAP_FIXTURES {
            let f = load_map_fixture(name);
            let m = parsed(&d, &f);
            for l in &f.los {
                assert_eq!(
                    los_f64(&m, l.a[0], l.a[1], l.b[0], l.b[1]),
                    l.ab,
                    "{name}: los {} -> {}",
                    l.from,
                    l.to
                );
                assert_eq!(
                    los_f64(&m, l.b[0], l.b[1], l.a[0], l.a[1]),
                    l.ba,
                    "{name}: los {} -> {}",
                    l.to,
                    l.from
                );
                // the f32 API agrees on these inputs too
                assert_eq!(
                    los(
                        &m,
                        l.a[0] as f32,
                        l.a[1] as f32,
                        l.b[0] as f32,
                        l.b[1] as f32
                    ),
                    l.ab
                );
                n += 1;
            }
        }
        assert!(n >= 100, "fixtures carry {n} LOS pairs");
    }

    #[test]
    fn nearest_reachable_matches_fixtures() {
        let d = data();
        for name in MAP_FIXTURES {
            let f = load_map_fixture(name);
            let m = parsed(&d, &f);
            let field = bfs_solid(&m, f.entry[0], f.entry[1]);
            for p in &f.nearest {
                let got = nearest_reachable(&m, &field, p.at[0], p.at[1])
                    .map(|i| i as i64)
                    .unwrap_or(-1);
                assert_eq!(got, p.idx, "{name}: nearest to {:?}", p.at);
            }
        }
    }

    #[test]
    fn lap_of_matches_fixtures() {
        let d = data();
        let f = load_map_fixture("source");
        let m = parsed(&d, &f);
        let laps = f.laps.as_ref().expect("source laps");
        for (i, &lap) in laps.iter().enumerate() {
            let (cx, cz) = m.cell_of(i);
            assert_eq!(lap_of_map(&m, cx, cz), lap, "lap at ({cx},{cz})");
        }
        let legacy = crate::fixtures::load_lap_legacy();
        assert_eq!((legacy.size, legacy.band, legacy.max_lap), (40, 3, 5));
        for (i, &lap) in legacy.laps.iter().enumerate() {
            assert_eq!(lap_of_legacy(i as i32 % 40, i as i32 / 40), lap);
        }
        assert_eq!(lap_of(0, 0, 40, 40, None), -1);
        assert_eq!(
            lap_of(
                30,
                30,
                60,
                60,
                Some(Bands {
                    band: 5,
                    max_lap: 5
                })
            ),
            5
        );
    }

    #[test]
    fn route_cells_match_fixtures() {
        let d = data();
        for name in MAP_FIXTURES {
            let f = load_map_fixture(name);
            let m = parsed(&d, &f);
            let r = f.route.as_ref().expect("route");
            let cmp = |got: Route, want: &crate::fixtures::RouteFixture, what: &str| {
                assert_eq!(
                    (got.cells, got.unreachable),
                    (want.cells, want.unreachable),
                    "{name}: route {what}"
                );
            };
            cmp(route_cells(&m, true, false), &r.closed, "closed");
            cmp(route_cells(&m, true, true), &r.open, "open");
            cmp(
                route_cells(&m, false, false),
                &r.gates_closed,
                "gates closed",
            );
        }
    }

    #[test]
    fn small_helpers() {
        let d = data();
        let m = d.parse_hub().expect("hub");
        assert_eq!(to_cell(&m, 70.5, 10.5), (10, 10));
        assert_eq!(center(&m, 10, 10), (70.5, 10.5));
        assert_eq!(cell_type(&m, -1, 0), CellKind::Wall);
        assert_eq!(cell_type(&m, 10, 10), CellKind::Stairs);
        assert!(is_extraction(CellKind::Elevator));
        assert!(!is_water(&m, 10, 10));
        assert!((dist2d(0.0, 0.0, 3.0, 4.0) - 5.0).abs() < 1e-6);
        let field = bfs_solid(&m, 10, 10);
        assert_eq!(field.dist_to(idx(&m, 10, 10)), Some(0));
        assert!(path_to(&m, &field, idx(&m, 10, 10)).is_empty());
        assert!(!field.reachable(idx(&m, 0, 0)));
        let mut pool = Pool::empty(m.len());
        pool.mask[idx(&m, 10, 9)] = 1;
        assert!(is_blocked(&m, &pool, 10, 9));
        assert!(!is_solid(&m, 10, 9));
    }
}
