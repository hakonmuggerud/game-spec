// main.js — owns the loop, the shared ctx, the event bus, the state machine, input, and the player
// (movement/lamp/flash/lantern/topUp/interact — v1 code moved here). DESIGN-v2 §9.
// Frame order: input → player → world → hunter → npc → contracts → hub → endgame → audio → ui → render.
import * as THREE from 'three';
import { CFG, KEYS, POINTS, TOOLS, TIERS, HUB_OX, CREATURE } from './config.js';
import * as MAPS from './maps.js';
import * as models from './models.js';
import * as world from './world.js';
import * as hunterMod from './hunter.js';
import * as npc from './npc.js';
import * as contracts from './contracts.js';
import * as hub from './hub.js';
import * as endgame from './endgame.js';
import * as ui from './ui.js';
import * as saveMod from './save.js';
import { audio } from './audio.js';

const { T, toCell, center, cellType, isSolid, dist2d } = MAPS;

/* ============================================================
   Event bus: on(name, fn) / off(name, fn) / emit(name, payload). Synchronous, try/catch per listener,
   errors go to console.error. '*' listeners receive (name, payload) for every event.
   ============================================================ */
function makeEvents() {
  const map = new Map();
  const list = (n) => { let l = map.get(n); if (!l) { l = []; map.set(n, l); } return l; };
  return {
    on(name, fn) { list(name).push(fn); return () => this.off(name, fn); },
    off(name, fn) { const l = map.get(name); if (!l) return; const i = l.indexOf(fn); if (i >= 0) l.splice(i, 1); },
    emit(name, payload = {}) {
      const l = map.get(name);
      if (l) for (const fn of l.slice()) { try { fn(payload, name); } catch (e) { console.error(`[events] ${name}:`, e); } }
      const any = map.get('*');
      if (any && name !== '*') for (const fn of any.slice()) { try { fn(name, payload); } catch (e) { console.error(`[events] * (${name}):`, e); } }
    },
    names() { return [...map.keys()]; },
  };
}

/* ============================================================
   ctx (DESIGN-v2 §9)
   ============================================================ */
const canvas = document.getElementById('c');
const renderer = new THREE.WebGLRenderer({ canvas, antialias: false, powerPreference: 'high-performance' });
renderer.setPixelRatio(1);
const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(CFG.fov, 1, 0.05, 40);
camera.rotation.order = 'YXZ';
scene.add(camera); // camera is in the graph so the lamp (its child) renders

const ctx = {
  THREE, scene, camera, renderer, canvas, dom: {}, cfg: CFG, maps: MAPS, models,
  state: { mode: 'TITLE', zoneId: null, paused: false, mouseFree: false, locked: false, lockEver: false, time: 0, menuKind: null, hintOverride: '',
    prevMode: null, dyingT: 0, lostLoot: 'nothing', lostAny: false },
  player: { x: 0, z: 0, yaw: 0, pitch: 0, oil: 50, lampOn: true, sprinting: false, moving: false,
    onDeep: false, inWater: false, inPool: false, lap: 0, carried: { oil: 0, relic: 0, rich: 0, quest: 0 },
    flashCd: 0, lanternCd: 0, flashT: 0, lampLock: 0, map: null, follower: null },   // lampLock: Lampwight relight lockout (s)
  hub: { map: null, group: null, flame: null, buildings: null },
  zone: { id: null, meta: null, map: null, group: null, spots: [], gates: [], npcCell: null },
  lanterns: [], items: [], hunters: [], npcs: [],
  save: saveMod.data, audio, ui, keys: new Set(), events: makeEvents(), actions: {},
};
// v1 compatibility views on ctx.state (window.__game.game): non-enumerable getters
Object.defineProperties(ctx.state, {
  state: { get: () => ctx.state.mode, set: (v) => { ctx.state.mode = v; } },
  points: { get: () => ctx.save.points, set: (v) => { ctx.save.points = v; } },
  bankedOil: { get: () => ctx.save.oil, set: (v) => { ctx.save.oil = v; } },
  tier: { get: () => (ctx.hub.flame ? ctx.hub.flame.tier : 1) },
  seenT: { get: () => 0 },
});
const { state, player, keys, events } = ctx;

const lamp = new THREE.PointLight(CFG.lampColor, CFG.lampInt, CFG.lampDist, 2);
lamp.position.set(0.25, -0.2, 0);
camera.add(lamp);

/* ============================================================
   Player controller + lamp
   ============================================================ */
function spawnAt(m, yaw) {
  const p = center(m, m.stairs.cx, m.stairs.cz);
  player.x = p.x; player.z = p.z; player.yaw = yaw; player.pitch = 0; player.map = m;
}
function updatePlayer(dt) {
  // keyboard look (fallback when pointer lock is unavailable)
  const rot = CFG.lookKeys * dt;
  if (keys.has(KEYS.lookLeft)) player.yaw += rot;
  if (keys.has(KEYS.lookRight)) player.yaw -= rot;
  if (keys.has(KEYS.lookUp)) player.pitch += rot;
  if (keys.has(KEYS.lookDown)) player.pitch -= rot;
  player.pitch = Math.max(-1.5, Math.min(1.5, player.pitch));

  // movement: W A S D, shift to sprint
  let mx = 0, mz = 0;
  if (keys.has(KEYS.forward)) mz -= 1;
  if (keys.has(KEYS.back)) mz += 1;
  if (keys.has(KEYS.left)) mx -= 1;
  if (keys.has(KEYS.right)) mx += 1;
  const moving = mx !== 0 || mz !== 0;
  player.sprinting = moving && KEYS.sprint.some(k => keys.has(k));
  if (moving) {
    const len = Math.hypot(mx, mz); mx /= len; mz /= len;
    const s = Math.sin(player.yaw), c = Math.cos(player.yaw);
    const vx = mx * c + mz * s, vz = -mx * s + mz * c;   // forward = (-sin, -cos)
    let speed = player.sprinting ? CFG.sprint : CFG.walk;
    if (player.inWater) speed *= player.sprinting ? CFG.waterSprintMul : CFG.waterWalkMul;
    world.moveWithCollision(player, vx * speed * dt, vz * speed * dt);
  }
  player.moving = moving;
  if (!Number.isFinite(player.x) || !Number.isFinite(player.z)) spawnAt(player.map, 0);

  // cell flags
  const c = toCell(player.map, player.x, player.z), t = cellType(player.map, c.cx, c.cz);
  player.onDeep = t === T.DEEP;
  player.inWater = t === T.WATER;
  player.lap = ctx.zone.meta && ctx.zone.meta.deepStyle === 'bands' && state.mode === 'ZONE' ? MAPS.lapOf(c.cx, c.cz, player.map) : 0;
  player.inPool = state.mode === 'ZONE' && ctx.lanterns.some(l => dist2d(l.x, l.z, player.x, player.z) <= CFG.poolR);

  // camera
  camera.position.set(player.x, CFG.eye, player.z);
  camera.rotation.set(player.pitch, player.yaw, 0);
  const targetFov = player.sprinting ? CFG.fovSprint : CFG.fov;
  camera.fov += (targetFov - camera.fov) * Math.min(1, dt * 8);
  camera.updateProjectionMatrix();
}
function zoneMul() { // zone/deep/light-tech multipliers for burn rate and lamp distance (v1 numbers for the Undercroft)
  const meta = ctx.zone.meta || {}, tech = hub.lightTech(), bands = meta.deepStyle === 'bands';
  const deepBurn = player.onDeep ? (bands ? CFG.bandBurn(player.lap) : CFG.deepBurnMul) : 1;
  const deepLamp = player.onDeep ? (bands ? CFG.bandLamp(player.lap) : CFG.deepLampMul) : 1;
  return { burn: deepBurn * (meta.burnMul || 1) * tech.burnMul, dist: deepLamp * (meta.lampMul || 1) * tech.distMul };
}
function updateLamp(dt, time) {
  // hub/title/dead: lamp forced off, no burn. It stays lit while DYING so the thing that took you
  // reads as a silhouette against the lit floor rather than two eye-glints in the black.
  if (state.mode !== 'ZONE' && state.mode !== 'DYING' && state.mode !== 'MENU' && state.mode !== 'ENDING') player.lampOn = false;
  const mul = zoneMul();
  if (player.lampOn) {
    player.oil -= CFG.burn * mul.burn * dt;
    if (player.oil <= 0) { player.oil = 0; player.lampOn = false; }
  }
  if (!Number.isFinite(player.oil)) player.oil = 0;
  player.flashCd = Math.max(0, player.flashCd - dt);
  player.lanternCd = Math.max(0, player.lanternCd - dt);
  player.flashT = Math.max(0, player.flashT - dt);
  player.lampLock = Math.max(0, (player.lampLock || 0) - dt);
  const amp = player.oil < 15 ? 0.07 * 3 : 0.07;
  const flick = 0.93 + amp * Math.sin(time * 13);
  let inten = player.lampOn ? CFG.lampInt * flick : 0;
  if (player.flashT > 0) inten = CFG.lampInt * CFG.flashMul;
  lamp.intensity = inten;
  lamp.visible = inten > 0;
  lamp.distance = CFG.lampDist * mul.dist;
}
function toggleLamp() {
  if (state.mode !== 'ZONE') return false;
  if (!player.lampOn && player.oil <= 0) { ui.toast('The lamp is dry.'); return false; }
  if (!player.lampOn && player.lampLock > 0) { ui.toast('The wick is cold'); events.emit('uiError', {}); return false; }   // DESIGN.md §5.1: snuffed
  player.lampOn = !player.lampOn;
  events.emit('lampToggle', { on: player.lampOn });
  return true;
}
function topUp() {
  if (state.mode !== 'ZONE' || player.carried.oil <= 0 || player.oil >= CFG.oilMax) return false;
  player.carried.oil -= 1;
  player.oil = Math.min(CFG.oilMax, player.oil + CFG.flaskOil);
  events.emit('topUp', { oil: player.oil });
  return true;
}

/* ============================================================
   Flash / planted lanterns
   ============================================================ */
function flash() {
  const cost = hub.lightTech().flashCost;
  if (state.mode !== 'ZONE' || player.flashCd > 0 || player.oil < cost) return false;
  player.oil -= cost; player.flashCd = CFG.flashCd; player.flashT = CFG.flashDur;
  const m = ctx.zone.map;
  for (const h of ctx.hunters) {
    if (!h.active) continue;
    const dx = h.x - player.x, dz = h.z - player.z, d = Math.hypot(dx, dz);
    if (d > 0 && d <= CFG.flashRange) {
      const dot = (dx * -Math.sin(player.yaw) + dz * -Math.cos(player.yaw)) / d;
      // the cone/LOS test is main's; what the flash does (stagger / flinch / sink / reveal / nothing) is the profile's
      if (dot > CFG.flashDot && MAPS.los(m, h.x, h.z, player.x, player.z)) hunterMod.onFlash(h);
    }
  }
  events.emit('flash', { x: player.x, z: player.z });
  return true;
}
function removeLantern(l) {
  world.removeLantern(l);
  events.emit('lanternRemoved', { x: l.x, z: l.z });
}
function plantLantern() {
  const cost = hub.lightTech().lanternCost;
  if (state.mode !== 'ZONE' || player.lanternCd > 0 || player.oil < cost) return false;
  player.oil -= cost; player.lanternCd = CFG.lanternCd;
  if (ctx.lanterns.length >= CFG.lanternMax) removeLantern(ctx.lanterns[0]);
  world.spawnLantern(player.x, player.z);
  events.emit('lantern', { x: player.x, z: player.z });
  return true;
}

/* ============================================================
   Transitions, hub/zone/bank/death
   ============================================================ */
const fade = { a: 0, mode: null, cb: null };
// a new transition may start during a fade-in, never during a fade-out (callback pending)
function transition(cb) { if (fade.mode === 'out') return false; fade.mode = 'out'; fade.cb = cb; return true; }
function updateFade(dt) {
  if (fade.mode === 'out') {
    fade.a = Math.min(1, fade.a + dt / CFG.fadeT);
    if (fade.a >= 1) { const cb = fade.cb; fade.cb = null; fade.mode = 'in'; if (cb) cb(); }
  } else if (fade.mode === 'in') {
    fade.a = Math.max(0, fade.a - dt / CFG.fadeT);
    if (fade.a <= 0) fade.mode = null;
  }
  ui.setFade(fade.a);
}
function describe(c) {
  const parts = [];
  if (c.oil) parts.push(`${c.oil} flask${c.oil === 1 ? '' : 's'}`);
  if (c.relic) parts.push(`${c.relic} relic${c.relic === 1 ? '' : 's'}`);
  if (c.rich) parts.push(`${c.rich} rich relic${c.rich === 1 ? '' : 's'}`);
  if (c.quest) parts.push(`${c.quest} quest item${c.quest === 1 ? '' : 's'}`);
  return parts.length ? parts.join(', ') : 'nothing';
}
const emptyCarried = () => ({ oil: 0, relic: 0, rich: 0, quest: 0 });

function enterHub() {
  state.mode = 'HUB';
  spawnAt(ctx.hub.map, 0);          // stairs are south of the flame; yaw 0 looks north at it
  player.lampOn = false; player.inPool = false; player.inWater = false; player.onDeep = false;
  events.emit('hubEnter', { zoneId: null });   // hunter: clear · hub: checkTier (→ flameTier → ui toast)
  // the HUD shows what the next run starts with, not the stale oil/cooldowns carried up from the zone
  player.oil = hub.startOil(); player.flashCd = 0; player.lanternCd = 0; player.flashT = 0;
}
// Load a zone into the scene without starting a run (title/hub); hunters are created inactive so
// window.__game.hunter is always a valid record.
function loadZoneInactive(id) {
  world.loadZone(id);
  hunterMod.spawnAll(ctx.zone); hunterMod.clear();
}
function startRun() {
  state.mode = 'ZONE';
  const zone = ctx.zone;
  spawnAt(zone.map, 0);
  player.oil = hub.startOil(); player.lampOn = true;
  player.flashCd = 0; player.lanternCd = 0; player.flashT = 0;
  world.resetItems(); world.clearLanterns();
  ctx.save.stats.runs += 1;
  events.emit('zoneEnter', { zoneId: zone.id });   // hunter: spawnAll + pools · npc/contracts/audio/ui
}
// descend(): stairs / tram / elevator all go to save.zoneSelected (DESIGN-v2 §7); a locked zone is refused here too,
// so a stale board selection can never start a run the player has not earned.
function selectedZone() { return ctx.save.zoneSelected in MAPS.ZONES ? ctx.save.zoneSelected : 'undercroft'; }
function descend() {
  if (state.mode !== 'HUB') return false;
  const id = selectedZone();
  // a zone that is already loaded was put there by actions.loadZone (the test hook) or an earlier permitted descent —
  // requirements are monotonic, so only an unloaded selection needs the lock check
  const lock = ctx.zone.id === id ? null : MAPS.zoneLocked(id, ctx.save, hub.tier());
  if (lock) { ui.toast(`${MAPS.ZONES[id].name}: ${lock}`); events.emit('uiError', {}); return false; }
  return transition(() => {
    if (ctx.zone.id !== id) loadZoneInactive(id);
    startRun();
  });
}
function bank() {
  if (state.mode !== 'ZONE') return false;
  const c = player.carried;
  const pts = (c.oil | 0) * POINTS.oil + (c.relic | 0) * POINTS.relic + (c.rich | 0) * POINTS.rich;
  const msg = pts > 0 ? `Banked: ${describe(c)} (+${pts})` : 'Nothing to bank. You return to the Lantern.';
  return transition(() => {
    const carried = { ...emptyCarried(), ...c }, zoneId = ctx.zone.id;
    ctx.save.points += pts; ctx.save.stats.banked += pts;
    player.carried = emptyCarried();
    events.emit('bank', { carried, pts, zoneId });   // hub: ledgers · contracts · npc · save · audio
    ui.toast(msg);
    events.emit('zoneExit', { zoneId });
    enterHub();
  });
}
function gateTarget() {
  const z = ctx.zone; if (!z.map || !z.gates.length) return null;
  const fx = -Math.sin(player.yaw), fz = -Math.cos(player.yaw);
  for (const g of z.gates) {
    if (g.open) continue;
    const p = center(z.map, g.cx, g.cz), dx = p.x - player.x, dz = p.z - player.z, d = Math.hypot(dx, dz);
    if (d <= CFG.interactR + 0.5 && dx * fx + dz * fz > 0) {
      const tool = z.meta.gate && z.meta.gate.tool, has = !!(tool && ctx.save.tools[tool]);
      return { type: 'gate', cx: g.cx, cz: g.cz, locked: !has, toolName: TOOLS[tool] || tool,
        // world.openGate refuses without the tool and emits gateLocked (audio plays the error cue); we add the toast
        run: () => { const ok = world.openGate(g.cx, g.cz); if (!ok && !has) ui.toast(`Locked — needs ${TOOLS[tool] || tool}`); return ok; } };
    }
  }
  return null;
}
// shortcutTarget(): the `=` door in reach (CFG.interactR + 0.5, and facing it). Mirrors gateTarget(), but a shortcut
// opens from ONE side only (DESIGN.md §3.6): from the far side E lifts the bars for good and toasts; from the
// barred side the hint reads "Barred from the other side" and E only beeps (uiError, no toast spam).
function shortcutTarget() {
  const z = ctx.zone; if (!z.map || !z.shortcuts || !z.shortcuts.length) return null;
  const fx = -Math.sin(player.yaw), fz = -Math.cos(player.yaw);
  for (const s of z.shortcuts) {
    if (s.open) continue;
    const p = center(z.map, s.cx, s.cz), dx = p.x - player.x, dz = p.z - player.z, d = Math.hypot(dx, dz);
    if (d > CFG.interactR + 0.5 || dx * fx + dz * fz <= 0) continue;
    const st = world.shortcutStatus(s, player.x, player.z);
    if (!st.canOpen) return { type: 'shortcut', id: s.id, name: s.name, cx: s.cx, cz: s.cz, barred: true,
      label: 'Barred from the other side', run: () => { events.emit('uiError', { reason: 'shortcutBarred', id: s.id }); return false; } };
    return { type: 'shortcut', id: s.id, name: s.name, cx: s.cx, cz: s.cz, barred: false, label: '[E] Lift the bars',
      run: () => { const ok = world.openShortcut(s.cx, s.cz); if (ok) ui.toast(`The bars fall. ${s.name} is open for good.`); return ok; } };
  }
  return null;
}
// interactTarget(): what E would do right now, in order: endgame → npc → hub → items → gates → shortcuts → stairs.
function interactTarget() {
  if (fade.mode === 'out' || state.paused) return null;
  if (state.mode !== 'ZONE' && state.mode !== 'HUB') return null;
  const m = player.map;
  const t = endgame.interactTarget(ctx) || npc.interactTarget(ctx) || hub.interactTarget(ctx);
  if (t) return t;
  if (state.mode === 'ZONE') {
    let best = null, bd = CFG.interactR;
    const fx = -Math.sin(player.yaw), fz = -Math.cos(player.yaw);
    for (const it of ctx.items) {
      const dx = it.x - player.x, dz = it.z - player.z, d = Math.hypot(dx, dz);
      if (d < bd && dx * fx + dz * fz > 0) { best = it; bd = d; }
    }
    if (best) return { type: 'item', item: best };
    const g = gateTarget(); if (g) return g;
    const sc = shortcutTarget(); if (sc) return sc;
  }
  if (m && m.stairs) {
    const s = center(m, m.stairs.cx, m.stairs.cz);
    if (dist2d(s.x, s.z, player.x, player.z) <= CFG.interactR) {
      if (state.mode === 'ZONE') return { type: 'bank', empty: describe(player.carried) === 'nothing' };
      if (state.mode === 'HUB') return { type: 'descend' };
    }
  }
  return null;
}
function pickup(it) {
  if (it.kind === 'bundle') {
    for (const k of ['oil', 'relic', 'rich', 'quest']) player.carried[k] = (player.carried[k] | 0) + (it.contents[k] | 0);
    ui.toast(`Recovered your bundle: ${describe(it.contents)}`);
  } else player.carried[it.kind] = (player.carried[it.kind] | 0) + 1;
  world.removeItem(it);
  events.emit('pickup', { kind: it.kind, item: it });
}
function interact() {
  const t = interactTarget();
  let handled = false;
  if (t) {
    if (t.type === 'item') { pickup(t.item); handled = true; }
    else if (t.type === 'bank') handled = bank();
    else if (t.type === 'descend') handled = descend();
    else if (typeof t.run === 'function') handled = t.run() !== false;
  }
  events.emit('interact', { target: t, handled, x: player.x, z: player.z });
  return handled;
}
function die(h) {
  if (state.mode !== 'ZONE') return false;
  h = h || ctx.hunters.find(x => x.active) || null;
  state.mode = 'DYING'; state.dyingT = CFG.dyingT;
  state.hintOverride = 'The dark took you.';
  const sum = (o) => (o.oil | 0) + (o.relic | 0) + (o.rich | 0) + (o.quest | 0);
  let c = { ...emptyCarried(), ...player.carried };
  if (sum(c) > 0) c = hub.applyBlessing(c);   // a blessed run banks ⌊half⌋ of each kind now; the rest drops
  const total = sum(c);
  state.lostLoot = describe(c);
  if (total > 0) {
    const old = ctx.items.find(i => i.kind === 'bundle');
    if (old) world.removeItem(old);
    world.spawnItem('bundle', player.x, player.z, c);
  }
  player.carried = emptyCarried();
  state.lostAny = total > 0;
  ctx.save.stats.deaths += 1;
  keys.clear();
  if (h) {
    // death cam: turn to face what took you, and step it back so it reads as a silhouette with eyes
    // rather than a point-blank slab (updatePlayer/hunter.update do not run while DYING, so set directly)
    const m = ctx.zone.map;
    const dx = h.x - player.x, dz = h.z - player.z, d = Math.hypot(dx, dz) || 1;
    const ux = dx / d, uz = dz / d;
    player.yaw = Math.atan2(-ux, -uz); player.pitch = 0;
    camera.rotation.set(0, player.yaw, 0);
    const bx = player.x + ux * 1.8, bz = player.z + uz * 1.8, bc = toCell(m, bx, bz);
    if (!isSolid(m, bc.cx, bc.cz)) { h.x = bx; h.z = bz; }
    h.yaw = Math.atan2(ux, uz);
    hunterMod.syncMesh(h);
  }
  releaseLock(); // so the death overlay can receive the click
  events.emit('death', { x: player.x, z: player.z, hunterId: h ? h.id : null, target: 'player' });
  return true;
}
function showDeathScreen() {
  state.mode = 'DEAD';
  ui.showDeath(state.lostLoot, state.lostAny);
  releaseLock();
}
function returnToHub() {
  if (state.mode !== 'DEAD' || fade.mode === 'out') return false;
  ui.hideDeath();
  state.hintOverride = '';
  requestLock();
  return transition(() => { events.emit('zoneExit', { zoneId: ctx.zone.id }); enterHub(); });
}
// begin(): Continue / New Game on the main menu (and actions.begin for tests): hide the menu, enter the hub, lock the pointer.
function begin() {
  if (state.mode !== 'TITLE') return false;
  ui.hideTitle();
  fade.a = 1; fade.mode = 'in';
  events.emit('begin', { zoneId: null });
  enterHub();
  requestLock();
  return true;
}
// Menus (MENU mode): release the pointer and pause the sim; ui draws, main owns the mode switch. `pause` is the
// pause menu (ui.showPause); every other kind is the generic #menu panel (board/build/service/dialog).
function openMenu(kind, opts) {
  if (state.mode !== 'HUB' && state.mode !== 'ZONE') return false;
  state.prevMode = state.mode; state.mode = 'MENU'; state.menuKind = kind;
  keys.clear(); releaseLock();
  if (kind === 'pause') ui.showPause(state.prevMode === 'ZONE');
  else ui.showMenu(opts || { title: kind });
  events.emit('menuOpen', { kind });
  return true;
}
function closeMenu() {
  if (state.mode !== 'MENU') return false;
  const kind = state.menuKind;
  if (kind === 'pause') ui.hidePause(); else ui.closeMenu();
  state.mode = state.prevMode || 'HUB'; state.prevMode = null; state.menuKind = null;
  requestLock();
  events.emit('menuClose', { kind });
  return true;
}
// Pause menu: Esc (or losing the pointer lock) in the hub or a zone. Never over another menu — openMenu refuses
// unless the mode is HUB/ZONE, so the board/build/dialog/ending screens and the pause menu cannot double-open.
// pauseGuardT: when the browser itself dropped the lock (the user's Esc, which Chrome may also deliver as a keydown
// a moment later), that keydown must not resume the menu it just opened; a keydown-opened pause needs no guard.
let pauseGuardT = 0;
function openPause(source = 'key') {
  if (state.mode !== 'HUB' && state.mode !== 'ZONE') return false;
  if (fade.mode === 'out') return false;
  if (!openMenu('pause')) return false;
  pauseGuardT = source === 'lock' ? performance.now() : 0;
  events.emit('pauseOpen', { inZone: state.prevMode === 'ZONE' });
  return true;
}
function closePause() {
  if (state.mode !== 'MENU' || state.menuKind !== 'pause') return false;
  return closeMenu();
}
// toMainMenu(): pause menu → main menu. From a zone the run is abandoned first: carried loot is lost (never banked),
// the hub is untouched. Also usable from the hub/zone directly (tests).
function toMainMenu() {
  if (state.mode === 'MENU') {
    const kind = state.menuKind;
    if (kind === 'pause') ui.hidePause(); else ui.closeMenu();
    state.mode = state.prevMode || 'HUB'; state.prevMode = null; state.menuKind = null;
    events.emit('menuClose', { kind });
  }
  if (state.mode === 'ZONE') {
    const lost = describe(player.carried), zoneId = ctx.zone.id;
    player.carried = emptyCarried();
    events.emit('runAbandoned', { zoneId, lost });
    events.emit('zoneExit', { zoneId });
    enterHub();
    state.lostBelow = lost !== 'nothing' ? lost : '';
  }
  if (state.mode !== 'HUB') return false;
  state.mode = 'TITLE'; state.hintOverride = ''; keys.clear(); releaseLock();
  fade.cb = null; fade.mode = 'in';
  spawnAt(ctx.hub.map, 0);                       // the main menu looks over the hub, as at boot
  camera.position.set(player.x, CFG.eye, player.z); camera.rotation.set(0, player.yaw, 0);
  ui.showTitle();
  events.emit('title', {});                      // ui flushes the run's toasts here …
  if (state.lostBelow) ui.toast(`Left below: ${state.lostBelow}.`);   // … so this one shows over the main menu
  state.lostBelow = '';
  return true;
}
// resetRuntime(): save.reset() plus every derived runtime state, without a reload: flame tier, hub buildings/ghosts,
// hub residents, contracts, tools/gates (the resident zone is reloaded), endings marks, minimap bitsets. Sound
// settings are preferences, not progress, and survive.
function resetRuntime() {
  const vol = audio.vol, muted = audio.muted;
  saveMod.reset();
  ctx.save.audio.vol = vol; ctx.save.audio.muted = muted;
  for (const id of Object.keys(MAPS.ZONES)) { const b = hub.exploredBits(id); if (b) b.fill(0); }   // hub.js caches these
  hub.checkTier(false);            // always re-emits flameTier {initial}: hub/world/npc/contracts/endgame re-derive
  hub.refresh();
  if (state.mode === 'TITLE' || state.mode === 'HUB' || (state.mode === 'MENU' && state.prevMode === 'HUB')) loadZoneInactive(selectedZone());
  if (state.mode === 'HUB' || state.mode === 'TITLE') player.oil = hub.startOil();
  ui.refreshMainMenu();
  events.emit('saveReset', {});
  return true;
}
// clearSave(): the pause menu's "Clear save" (after its confirm) — abandon any run, wipe, back to the main menu.
function clearSave() {
  if (state.mode === 'MENU' || state.mode === 'ZONE' || state.mode === 'HUB') { if (!toMainMenu()) return false; }
  if (state.mode !== 'TITLE') return false;
  resetRuntime();
  ui.toast('Save wiped.');
  return true;
}
// newGame(force): main menu "New Game". With progress in the save it asks first (ui confirm panel → newGame(true)).
function newGame(force = false) {
  if (state.mode !== 'TITLE') return false;
  if (saveMod.hasProgress() && !force) return ui.confirmNewGame() ? 'confirm' : false;
  if (saveMod.hasProgress()) resetRuntime();
  return begin();
}

/* ============================================================
   Pointer lock + mouse look + keyboard
   ============================================================ */
function requestLock() {
  try {
    const r = canvas.requestPointerLock && canvas.requestPointerLock();
    if (r && typeof r.catch === 'function') r.catch(() => { state.locked = false; });
  } catch (e) { state.locked = false; }
}
function releaseLock() {
  try { if (document.pointerLockElement === canvas && document.exitPointerLock) document.exitPointerLock(); } catch (e) { /* ignore */ }
}
document.addEventListener('pointerlockchange', () => {
  state.locked = document.pointerLockElement === canvas;
  if (state.locked) state.lockEver = true;
  // the browser drops the lock on Esc (swallowing the key), on alt-tab, on focus loss: that is a pause
  else if (state.mode === 'HUB' || state.mode === 'ZONE') openPause('lock');
});
document.addEventListener('pointerlockerror', () => { state.locked = false; });
document.addEventListener('mousemove', e => {
  if (!state.locked || !(state.mode === 'HUB' || state.mode === 'ZONE' || state.mode === 'DYING')) return;
  const mx = e.movementX || 0, my = e.movementY || 0;
  if (!Number.isFinite(mx) || !Number.isFinite(my)) return;
  player.yaw -= mx * CFG.mouseSens;
  player.pitch = Math.max(-1.5, Math.min(1.5, player.pitch - my * CFG.mouseSens));
});
canvas.addEventListener('click', () => {
  if (state.mode === 'HUB' || state.mode === 'ZONE') requestLock();
  else if (state.mode === 'DEAD') returnToHub(); // a click retargeted to the canvas while still pointer-locked
});

let resetArmedT = 0;
window.addEventListener('keydown', e => {
  if (['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Space', 'Tab'].includes(e.code)) e.preventDefault();
  keys.add(e.code);
  if (e.repeat) return;
  const go = KEYS.confirm.includes(e.code);
  const mode = state.mode;
  if (mode === 'TITLE') {
    // main menu: ↑↓ / W S, Enter / Space / E, 1–4, Esc backs out of a sub-panel; Backspace ×2 stays as a hidden wipe
    if (e.code === KEYS.reset) {
      if (resetArmedT > 0) { ctx.actions.reset(); ui.toast('Save wiped.'); resetArmedT = 0; }
      else { resetArmedT = 3; ui.toast('Press again to wipe the save'); }
    } else ui.mainMenuKey(e.code);
    events.emit('key', { code: e.code, mode });
    return;
  }
  if (mode === 'DEAD') { if (go) returnToHub(); events.emit('key', { code: e.code, mode }); return; }
  if (mode === 'MENU') {
    if (state.menuKind === 'pause') {
      // a real browser fires pointerlockchange AND may deliver the Esc that caused it: don't let that one resume
      if (!(e.code === KEYS.menuClose && pauseGuardT && performance.now() - pauseGuardT < 250)) ui.pauseKey(e.code);
    } else if (e.code === KEYS.menuClose) closeMenu();
    events.emit('key', { code: e.code, mode });
    return;
  }
  if (e.code === KEYS.mute) audio.toggleMute();
  else if (e.code === KEYS.volDown) audio.setVolume(audio.vol - 0.1);
  else if (e.code === KEYS.volUp) audio.setVolume(audio.vol + 0.1);
  if (e.code === KEYS.menuClose && (mode === 'HUB' || mode === 'ZONE')) { openPause(); events.emit('key', { code: e.code, mode }); return; }
  if (state.paused || fade.mode === 'out' || mode === 'ENDING' || mode === 'DYING') { events.emit('key', { code: e.code, mode }); return; }
  switch (e.code) {
    case KEYS.lamp: toggleLamp(); break;
    case KEYS.flash: flash(); break;
    case KEYS.lantern: plantLantern(); break;
    case KEYS.interact: interact(); break;
    case KEYS.topUp: topUp(); break;
    case KEYS.minimap: ui.toggleMinimap(); break;
    default: break;
  }
  events.emit('key', { code: e.code, mode });
});
window.addEventListener('keyup', e => keys.delete(e.code));
window.addEventListener('blur', () => keys.clear());

/* ============================================================
   Main loop + resize
   ============================================================ */
function resize() {
  const w = Math.max(1, window.innerWidth | 0), h = Math.max(1, window.innerHeight | 0);
  renderer.setSize(Math.max(1, Math.floor(w / 3)), Math.max(1, Math.floor(h / 3)), false);
  camera.aspect = w / h; camera.updateProjectionMatrix();
}
window.addEventListener('resize', resize);

/* ============================================================
   Screen fx: ctx.fx.shake(amount) — a pitch jitter `amount·sin(28t)` decaying over 0.25 s plus a 0.01 u eye dip
   (DESIGN.md §5.5: Brute strides within 8 u). Applied after every module has run, just before the render.
   ============================================================ */
const fx = { amp: 0, t: 0, dur: 0.25, pitch: 0, shake(amount) { if (!(amount > 0)) return; fx.amp = Math.max(fx.amp, amount); fx.t = fx.dur; } };
ctx.fx = fx;
function applyFx(dt) {
  fx.pitch = 0;
  if (fx.t <= 0) { fx.amp = 0; return; }
  fx.t = Math.max(0, fx.t - dt);
  const k = fx.t / fx.dur;
  fx.pitch = fx.amp * k * Math.sin(28 * state.time);
  camera.rotation.x += fx.pitch;
  camera.position.y -= 0.01 * k;
}

function update(dt) {
  const time = state.time, mode = state.mode;
  const playing = mode === 'HUB' || mode === 'ZONE';
  // Pausing is the pause menu (MENU mode): losing the pointer lock opens it. Being unlocked while playing only
  // happens after a Resume whose re-lock lagged or was refused — the sim keeps running and the HUD asks for a click.
  state.paused = false;
  state.mouseFree = playing && state.lockEver && !state.locked;
  resetArmedT = Math.max(0, resetArmedT - dt);
  updateFade(dt);
  if (playing && !state.paused) {
    updatePlayer(dt);
    updateLamp(dt, time);
  } else if (mode === 'DYING') {
    updateLamp(dt, time);
    state.dyingT -= dt;
    if (state.dyingT <= 0) showDeathScreen();
  } else {
    updateLamp(0, time);
  }
  world.update(ctx, dt);
  hunterMod.update(ctx, dt);
  npc.update(ctx, dt);
  contracts.update(ctx, dt);
  hub.update(ctx, dt);
  endgame.update(ctx, dt);
  audio.update(ctx, dt);
  ui.update(ctx, dt);
  applyFx(dt);
}
const clock = new THREE.Clock();
function frame() {
  requestAnimationFrame(frame);
  let dt = clock.getDelta();
  if (!Number.isFinite(dt) || dt < 0) dt = 1 / 60;
  dt = Math.min(dt, 0.05);
  state.time += dt;
  update(dt);
  renderer.render(scene, camera);
}

/* ============================================================
   Actions (shared by input, tests and other modules via ctx.actions)
   ============================================================ */
Object.assign(ctx.actions, {
  begin, descend, bank, flash, plantLantern, interact, interactTarget, topUp, toggleLamp, returnToHub, die,
  shortcutTarget, gateTarget,
  enterHub, openMenu, closeMenu, transition, spawnAt, describe,
  // menus (DESIGN §2): main menu, pause menu, save clearing
  openMainMenu: toMainMenu, toMainMenu, openPause, closePause, clearSave, newGame,
  saveInfo: () => ({ hasProgress: saveMod.hasProgress(), ...saveMod.summary() }),
  mainMenu: () => ui.mainMenuState(), pauseMenu: () => ui.pauseState(),
  pickWander: (far, h) => hunterMod.pickWander(h || ctx.hunters[0], !!far),
  los: MAPS.los, bfsField: MAPS.bfsField,
  // v2 test hooks (DESIGN-v2 §9)
  loadZone(id) {
    if (!MAPS.ZONES[id]) return false;
    hub.select(id);
    if (state.mode === 'ZONE' || state.mode === 'DYING') { events.emit('zoneExit', { zoneId: ctx.zone.id }); loadZoneInactive(id); startRun(); }
    else if (ctx.zone.id !== id) loadZoneInactive(id);
    return true;
  },
  selectZone: (id) => hub.select(id),
  freeNpc: (id) => npc.free(id),
  accept: (id) => contracts.accept(id),
  build: (id, opts) => hub.build(id, opts),
  choose: (id) => endgame.choose(id),
  giveTool(id) { if (!(id in ctx.save.tools)) return false; if (ctx.save.tools[id]) return true; ctx.save.tools[id] = true; events.emit('toolGained', { id, reward: { tool: id } }); return true; },
  setPoints(n) { ctx.save.points = Math.max(0, n | 0); hub.checkTier(true); return ctx.save.points; },
  setFlamePoints(n) { return ctx.actions.setPoints(n); },
  // setResources({oil, relics, rich}): banked ledgers (absolute values; omitted keys keep theirs)
  setResources(r = {}) { for (const k of ['oil', 'relics', 'rich']) if (Number.isFinite(r[k])) ctx.save[k] = Math.max(0, r[k] | 0); hub.refresh(); return { oil: ctx.save.oil, relics: ctx.save.relics, rich: ctx.save.rich }; },
  // rescue(id): mark an NPC rescued as if banked with them (npc.js re-places the hub residents; hub reveals the ghost)
  rescue(id) {
    if (!(id in ctx.save.rescued)) return false;
    if (ctx.save.rescued[id]) return true;
    ctx.save.rescued[id] = true; ctx.save.stats.rescues += 1;
    events.emit('npcRescued', { id, zoneId: ctx.zone.id, debug: true });
    return true;
  },
  // unlockAll(): every NPC rescued, tool owned, building built, light-tech III, tier 4, plenty of resources
  unlockAll() {
    const s = ctx.save;
    for (const k of Object.keys(s.rescued)) s.rescued[k] = true;
    for (const k of Object.keys(s.tools)) s.tools[k] = true;
    for (const k of Object.keys(s.buildings)) s.buildings[k] = true;
    const prevTech = s.lightTech | 0; s.lightTech = 3;
    s.points = Math.max(s.points | 0, TIERS[TIERS.length - 1].pts);   // tier 4
    s.oil = Math.max(s.oil | 0, 999); s.relics = Math.max(s.relics | 0, 99); s.rich = Math.max(s.rich | 0, 20);
    hub.checkTier(false);            // re-emits flameTier {initial}: hub/npc/contracts/endgame re-derive from the save
    hub.refresh();
    if (prevTech !== 3) events.emit('lightTech', { tier: 3, prev: prevTech });
    return true;
  },
  // gotoZone(id): start a run in that zone right now (from the title, hub, a zone, or the death screen) — no fade,
  // no lock check. `loadZone` (above) keeps its v2 semantics: in the hub it only loads the zone inactive.
  gotoZone(id) {
    if (!MAPS.ZONES[id]) return false;
    if (state.mode === 'TITLE') begin();
    if (state.mode === 'MENU') closeMenu();
    if (state.mode === 'ENDING') { if (!endgame.cancel()) return false; }
    if (state.mode === 'DEAD') { ui.hideDeath(); state.hintOverride = ''; }
    fade.cb = null; fade.mode = 'in'; fade.a = Math.max(fade.a, 0.6);
    if (state.mode === 'ZONE' || state.mode === 'DYING' || state.mode === 'DEAD') events.emit('zoneExit', { zoneId: ctx.zone.id });
    state.hintOverride = '';
    hub.select(id);
    loadZoneInactive(id);
    startRun();
    return true;
  },
  // teleport(x, z, yaw?): move the player within the current map (tests)
  teleport(x, z, yaw) { player.x = x; player.z = z; if (Number.isFinite(yaw)) player.yaw = yaw; return { x: player.x, z: player.z }; },
  spawnHunter: (cx, cz, profile) => hunterMod.spawnHunter(cx, cz, profile),
  // spawnCreature(profile, cx, cz, opts): any creature profile at a cell (DESIGN.md §5.8 debug API)
  spawnCreature: (profile, cx, cz, opts) => hunterMod.spawnCreature(profile, cx, cz, opts),
  creature: (profile, cx, cz, opts) => hunterMod.spawnCreature(profile, cx, cz, opts),
  shake: (amount) => fx.shake(amount),
  rideUp: () => endgame.rideUp(),
  openChoice: () => endgame.openChoice(),
  continueEnding: () => endgame.continueToHub(),
  reset: resetRuntime,
});

/* ============================================================
   Init (order matters: listeners must exist before hub.init emits the initial flameTier)
   ============================================================ */
saveMod.load();
saveMod.init(ctx);
events.on('hunterCatch', ({ hunterId, target }) => { if (target === 'player') die(ctx.hunters[hunterId]); });
// DESIGN.md §5.1: the Lampwight's touch puts the lamp out, burns oil and locks the wick for `lockout` seconds
events.on('lampSnuffed', ({ oil, lockout }) => {
  if (state.mode !== 'ZONE') return;
  player.lampOn = false;
  player.oil = Math.max(0, player.oil - (oil || CREATURE.lampwight.oil));
  player.lampLock = Math.max(player.lampLock || 0, lockout || CREATURE.lampwight.lockout);
});
// DESIGN.md §5.5: a Brute stride within shakeR shakes the camera
events.on('creatureStep', ({ d }) => { const R = CREATURE.brute.shakeR; if (state.mode === 'ZONE' && d <= R) fx.shake(CREATURE.brute.shakeAmp * (1 - d / R)); });
ui.init(ctx);
world.init(ctx);                                   // fog/ambient, hub blocks + sconces, flameTier listener
hunterMod.init(ctx);
loadZoneInactive(ctx.save.zoneSelected in MAPS.ZONES ? ctx.save.zoneSelected : 'undercroft');
audio.init(ctx);
contracts.init(ctx);                               // sets ctx.contracts (npc.talk offers contracts through it)
npc.init(ctx);
hub.init(ctx);                                     // flame + initial tier (emits flameTier {initial: true})
endgame.init(ctx);
resize();
spawnAt(ctx.hub.map, 0);                           // title screen looks over the hub
camera.position.set(player.x, CFG.eye, player.z);
ui.showTitle();                                    // main menu (Continue / New Game / Controls / Sound)
frame();

/* ============================================================
   Debug / test API — every v1 field kept, plus ctx and the v2 lists
   ============================================================ */
window.__game = {
  ctx,
  get state() { return state.mode; },
  game: state, player, keys, cfg: CFG, events, audio, actions: ctx.actions, dom: ctx.dom,
  get hunter() { return ctx.hunters[0]; },
  get hunters() { return ctx.hunters; },
  get npcs() { return ctx.npcs; },
  get flame() { return ctx.hub.flame; },
  get map() { return ctx.zone.map; },
  get maps() { return { zone: ctx.zone.map, hub: ctx.hub.map }; },
  get zone() { return ctx.zone; },
  get lanterns() { return ctx.lanterns; },
  get items() { return ctx.items; },
  get save() { return ctx.save; },
  // v2 module surfaces (each module also mirrors its own after init; these are the same objects)
  get world() { return ctx.world; },
  get hub() { return ctx.hub.api; },
  get npc() { return ctx.npc; },
  get contracts() { return ctx.contracts; },
  get endgame() { return ctx.endgame; },
  get hunterApi() { return { spawnAll: () => hunterMod.spawnAll(ctx.zone), clear: hunterMod.clear, stagger: hunterMod.stagger, investigate: hunterMod.investigate, recomputePools: hunterMod.recomputePools,
    onFlash: hunterMod.onFlash, hint: hunterMod.hint, info: hunterMod.info, spawnCreature: hunterMod.spawnCreature, profiles: hunterMod.PROFILES, pickWander: hunterMod.pickWander }; },
  get fx() { return fx; },
  models, mapsApi: MAPS,
  HUB_OX,
};
window.__proto = window.__game;
