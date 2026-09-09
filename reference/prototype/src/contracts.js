// contracts.js — NPC contracts (DESIGN-v2 §4).
// Each rescued NPC posts one contract at a time (its list in CONTRACT_ORDER; done → next). Accepted in the NPC
// dialogue (npc.js calls ctx.contracts.available(npcId) and ctx.actions.accept(id)); up to MAX_ACTIVE active.
// State per contract: 'locked' (poster not rescued) → 'available' → 'active' → 'done'. Progress survives in
// ctx.save.contracts.{active,done,progress}; a death or an early bank resets run-scoped progress (fetch/survive/
// recover) but never removes the contract — that is reported as `contractFailed {id, reason}` and the contract
// stays active.
// Types: plant (lantern ≤ plantR of the spot), fetch (bank ≥ n of kind from the zone in one run; progress =
// carried while in that zone), recover (a `quest` item spawns at the spot; pick it up and bank it), survive
// (contiguous seconds ≤ spotR of the spot; lampOff variants need the lamp doused; the timer resets on leaving).
// Listens: pickup, bank, lantern (+ lanternPlanted alias), death, zoneEnter, zoneExit, npcRescued, flameTier (reset).
// Emits: contractAccepted, contractComplete, contractFailed, contractProgress, toolGained (via actions.giveTool),
// uiClick, toast. HUD: ui.setContractLines (falls back to ui.setObjective, then a #objective element).
// Only config.js / maps.js may be imported; everything else is reached through ctx (feature-detected).
import { CFG, TOOLS, LABEL } from './config.js';
import { ZONES, center, dist2d } from './maps.js';

let ctx = null;

// Tuning (DESIGN-v2 §4/§10). Kept here so the numbers travel with the module.
export const CONTRACT_CFG = {
  maxActive: 2,     // contracts carried at once
  spotR: 3,         // survive: within this of the spot
  plantR: 1.5,      // plant: lantern within this of the spot
  questY: 0.5,      // fallback quest item height (world.js ITEM_STYLE.quest.y)
};
export const MAX_ACTIVE = CONTRACT_CFG.maxActive;

export const CONTRACTS = {
  c_relight: { poster: 'lamplighter', type: 'plant', zone: 'undercroft', spot: 1, title: 'Relight the Great Hall',
    text: 'Plant a lantern in the great hall, dead centre. Let it see the dark can be pushed.', reward: { tool: 'prybar', pts: 4 } },
  c_wick:    { poster: 'lamplighter', type: 'fetch', zone: 'undercroft', kind: 'oil', n: 4, title: "Wick's Reserve",
    text: 'Bring four flasks up from the Undercroft in a single run. No sipping on the way.', reward: { oil: 60 } },
  c_sound:   { poster: 'cartographer', type: 'survive', zone: 'cistern', spot: 0, seconds: 30, lampOff: false, title: 'Sound the Drowned Hall',
    text: 'Stand in the middle of the drowned hall for thirty seconds so I can hear how the water moves.', reward: { tool: 'sluice' } },
  c_chart:   { poster: 'cartographer', type: 'recover', zone: 'cistern', spot: 1, item: 'lost chart', title: 'The Lost Chart',
    text: 'I dropped my chart in the pump room when they took me. Bring it up.', reward: { pts: 8 } },
  c_censer:  { poster: 'keeper', type: 'fetch', zone: 'ossuary', kind: 'rich', n: 2, title: "Censer's Due",
    text: 'Two rich relics from the Ossuary, banked together. The press needs something worth burning.', reward: { tool: 'censer' } },
  c_ledger:  { poster: 'keeper', type: 'recover', zone: 'ossuary', spot: 0, item: 'ledger', title: "The Keeper's Ledger",
    text: 'My ledger lies in the east bone-pit. Every debt this vault owes is in it.', reward: { oil: 80 } },
  c_vigil:   { poster: 'deacon', type: 'survive', zone: 'ossuary', spot: 1, seconds: 45, lampOff: true, title: 'Vigil in the Dark',
    text: 'Keep vigil in the south vault with your lamp doused. Forty-five seconds. Listen to it listen.', reward: { pts: 10 } },
  c_bones:   { poster: 'deacon', type: 'fetch', zone: 'ossuary', kind: 'relic', n: 3, title: 'Bones for the Deacon',
    text: 'Three relics from the Ossuary in one run. They were prayers once; let them be fuel.', reward: { pts: 6 } },
};
// Posting order per NPC (DESIGN-v2 §4 table order).
export const CONTRACT_ORDER = Object.keys(CONTRACTS);
for (const id of CONTRACT_ORDER) CONTRACTS[id].id = id;

/* ============================================================
   Helpers
   ============================================================ */
const store = () => {
  const s = ctx.save;
  if (!s.contracts || typeof s.contracts !== 'object') s.contracts = { active: [], done: [], progress: {} };
  const c = s.contracts;
  if (!Array.isArray(c.active)) c.active = [];
  if (!Array.isArray(c.done)) c.done = [];
  if (!c.progress || typeof c.progress !== 'object') c.progress = {};
  return c;
};
// Toasts go through the event bus (ui.js queues `toast`), so tests and audio can observe them.
const toast = (msg) => { if (ctx.events) ctx.events.emit('toast', { msg }); else if (ctx.ui && typeof ctx.ui.toast === 'function') ctx.ui.toast(msg); };
const zoneName = (id) => (ZONES[id] && ZONES[id].name) || id;
const goalOf = (c) => (c.type === 'fetch' ? c.n : c.type === 'survive' ? c.seconds : 1);
const inZone = (c) => ctx.state.mode === 'ZONE' && ctx.zone && ctx.zone.id === c.zone;

// World position of a contract spot (zone maps sit at ox 0; prefer the parsed map when that zone is loaded).
export function spotPos(id) {
  const c = CONTRACTS[id];
  if (!c || c.spot == null) return null;
  const m = ctx && ctx.zone && ctx.zone.id === c.zone ? ctx.zone.map : null;
  const s = m && m.spots && m.spots[c.spot];
  if (s) return center(m, s.cx, s.cz);
  const meta = ZONES[c.zone], d = meta && meta.spots && meta.spots[c.spot];
  if (d) return { x: d.cell[0] + 0.5, z: d.cell[1] + 0.5 };
  return null;
}
export function rewardText(r) {
  const parts = [];
  if (r.tool) parts.push(TOOLS[r.tool] || r.tool);
  if (r.pts) parts.push(`${r.pts} flame point${r.pts === 1 ? '' : 's'}`);
  if (r.oil) parts.push(`${r.oil} oil`);
  return parts.join(', ') || 'nothing';
}
function kindLabel(kind, n) {
  const base = LABEL[kind] || kind;
  return n === 1 ? base : `${base}s`;
}
export function objectiveText(id) {
  const c = CONTRACTS[id]; if (!c) return '';
  const spot = c.spot != null && ZONES[c.zone] && ZONES[c.zone].spots[c.spot] ? ZONES[c.zone].spots[c.spot].label : 'the spot';
  switch (c.type) {
    case 'plant': return `Plant a lantern in ${spot} of ${zoneName(c.zone)}.`;
    case 'fetch': return `Bank ${c.n} ${kindLabel(c.kind, c.n)} from ${zoneName(c.zone)} in one run.`;
    case 'recover': return `Recover the ${c.item} from ${spot} of ${zoneName(c.zone)} and bank it.`;
    case 'survive': return `Stay ${c.seconds} s in ${spot} of ${zoneName(c.zone)}${c.lampOff ? ' with the lamp doused' : ''}.`;
    default: return '';
  }
}

/* ============================================================
   State machine
   ============================================================ */
export function status(id) {
  if (!CONTRACTS[id] || !ctx) return null;
  const s = store();
  if (s.done.includes(id)) return 'done';
  if (s.active.includes(id)) return 'active';
  const c = CONTRACTS[id];
  if (!(ctx.save.rescued && ctx.save.rescued[c.poster])) return 'locked';
  return available(c.poster) && available(c.poster).id === id ? 'available' : 'queued';
}
export function progress(id) { return ctx ? (store().progress[id] || 0) : 0; }
export function active() { return ctx ? store().active.slice() : []; }
export function done() { return ctx ? store().done.slice() : []; }
export function get(id) { return CONTRACTS[id] || null; }
// available(npcId): the next contract this NPC can post — {id, title, text, objective, reward, rewardText, zone,
// type} — or null (not rescued, one already active for this NPC, or list exhausted).
export function available(npcId) {
  if (!ctx || !npcId) return null;
  if (!(ctx.save.rescued && ctx.save.rescued[npcId])) return null;
  const s = store();
  for (const id of CONTRACT_ORDER) {
    const c = CONTRACTS[id];
    if (c.poster !== npcId || s.done.includes(id)) continue;
    if (s.active.includes(id)) return null;     // one contract per NPC at a time
    return { id, title: c.title, text: c.text, objective: objectiveText(id), reward: { ...c.reward }, rewardText: rewardText(c.reward), zone: c.zone, type: c.type };
  }
  return null;
}
// accept(id, {force}): true if accepted (poster rescued, next in its list, ≤ MAX_ACTIVE active). Emits contractAccepted.
export function accept(id, opts = {}) {
  if (!ctx) return false;
  const c = CONTRACTS[id]; if (!c) return false;
  const s = store();
  if (s.done.includes(id) || s.active.includes(id)) return false;
  if (!opts.force) {
    const offer = available(c.poster);
    if (!offer || offer.id !== id) return false;
  }
  if (s.active.length >= CONTRACT_CFG.maxActive) { toast('You already carry enough contracts.'); ctx.events.emit('uiError', {}); return false; }
  s.active.push(id);
  s.progress[id] = 0;
  ctx.events.emit('contractAccepted', { id, reward: { ...c.reward }, contract: c });
  ctx.events.emit('uiClick', {});
  toast(`Contract accepted: ${c.title}`);
  if (c.type === 'recover' && inZone(c)) spawnQuest(c);
  refreshHud(true);
  return true;
}
// abandon(id): drop an active contract (debug/menu); it becomes available again from its poster.
export function abandon(id) {
  const s = store(); const i = s.active.indexOf(id);
  if (i < 0) return false;
  s.active.splice(i, 1); delete s.progress[id];
  ctx.events.emit('contractFailed', { id, reason: 'abandoned' });
  refreshHud(true);
  return true;
}
function grant(c) {
  const r = c.reward || {}, save = ctx.save, A = ctx.actions || {};
  if (r.pts) {
    if (typeof A.setPoints === 'function') A.setPoints((save.points | 0) + r.pts);   // re-tiers the flame (hub.checkTier)
    else save.points = (save.points | 0) + r.pts;
  }
  if (r.oil) save.oil = (save.oil | 0) + r.oil;
  if (r.tool) {
    if (save.tools && !save.tools[r.tool] && typeof A.giveTool === 'function') A.giveTool(r.tool);   // emits toolGained
    else { if (!save.tools) save.tools = {}; save.tools[r.tool] = true; ctx.events.emit('toolGained', { id: r.tool, reward: { tool: r.tool } }); }
  }
}
function complete(id) {
  const c = CONTRACTS[id], s = store();
  const i = s.active.indexOf(id);
  if (i < 0) return false;
  s.active.splice(i, 1);
  if (!s.done.includes(id)) s.done.push(id);
  s.progress[id] = goalOf(c);
  grant(c);
  toast(`Contract complete: ${c.title} — ${rewardText(c.reward)}`);
  ctx.events.emit('contractComplete', { id, reward: { ...c.reward }, contract: c });
  refreshHud(true);
  return true;
}
function setProgress(id, v, emit = true) {
  const s = store(), old = s.progress[id] || 0;
  if (old === v) return;
  s.progress[id] = v;
  if (emit) ctx.events.emit('contractProgress', { id, progress: v, goal: goalOf(CONTRACTS[id]) });
}
// Reset run-scoped progress (death / early bank / leaving the zone). Emits contractFailed when something was lost.
function resetRun(reason, zoneId) {
  const s = store();
  for (const id of s.active.slice()) {
    const c = CONTRACTS[id]; if (!c) continue;
    if (c.type === 'plant') continue;
    if (zoneId && c.zone !== zoneId) continue;
    const had = s.progress[id] || 0;
    if (had > 0) {
      s.progress[id] = 0;
      ctx.events.emit('contractFailed', { id, reason, progress: had, goal: goalOf(c) });
      if (reason === 'death') toast(`Contract progress lost: ${c.title}`);
    }
  }
  refreshHud(true);
}

/* ============================================================
   Quest items (recover)
   ============================================================ */
function findQuestItem(id) { return ctx.items.find(it => it.kind === 'quest' && it.questId === id) || null; }
function bundleHoldsQuest() { return ctx.items.some(it => it.kind === 'bundle' && it.contents && (it.contents.quest | 0) > 0); }
function spawnQuest(c) {
  if (findQuestItem(c.id) || bundleHoldsQuest() || (ctx.player.carried.quest | 0) > 0) return null;
  const p = spotPos(c.id); if (!p) return null;
  let it = null;
  const W = ctx.world;
  if (W && typeof W.spawnItem === 'function') { try { it = W.spawnItem('quest', p.x, p.z, { contract: c.id }); } catch (e) { it = null; } }
  if (!it) {   // fallback: same record shape as world.spawnItem so main.pickup/world.removeItem/world.update handle it
    const THREE = ctx.THREE;
    const mesh = new THREE.Mesh(new THREE.TetrahedronGeometry(0.35),
      new THREE.MeshLambertMaterial({ color: 0xe0d0a0, emissive: 0xe0d0a0, emissiveIntensity: 0.6 }));
    mesh.position.set(p.x, CONTRACT_CFG.questY, p.z);
    ctx.scene.add(mesh);
    it = { kind: 'quest', x: p.x, z: p.z, mesh, baseY: CONTRACT_CFG.questY, phase: Math.random() * 6.28, contents: { contract: c.id } };
    ctx.items.push(it);
  }
  it.questId = c.id; it.label = c.item;
  return it;
}
function spawnQuestsFor(zoneId) {
  for (const id of store().active) {
    const c = CONTRACTS[id];
    if (c && c.type === 'recover' && c.zone === zoneId) spawnQuest(c);
  }
}

/* ============================================================
   HUD
   ============================================================ */
let hudCache = '';
let objectiveEl = null;
export function hudLines() {
  if (!ctx) return [];
  const s = store(), out = [];
  for (const id of s.active) {
    const c = CONTRACTS[id]; if (!c) continue;
    const g = goalOf(c), p = Math.min(g, Math.floor(s.progress[id] || 0));
    out.push(`◇ ${c.title} — ${p}/${g}${c.type === 'survive' ? ' s' : ''}`);
  }
  return out;
}
function ensureObjectiveEl() {
  if (objectiveEl || typeof document === 'undefined') return objectiveEl;
  objectiveEl = document.getElementById('objective');
  if (!objectiveEl) {
    objectiveEl = document.createElement('div');
    objectiveEl.id = 'objective';
    objectiveEl.style.cssText = 'white-space:pre-line;color:#a89878;font-size:12px;';
    const host = (ctx.dom && (ctx.dom.tr || ctx.dom.hud)) || document.getElementById('tr') || document.getElementById('hud') || document.body;
    host.appendChild(objectiveEl);
  }
  return objectiveEl;
}
function refreshHud(force = false) {
  const lines = hudLines(), key = lines.join('\n');
  if (!force && key === hudCache) return;
  hudCache = key;
  const ui = ctx.ui;
  if (ui && typeof ui.setContractLines === 'function') ui.setContractLines(lines);
  else if (ui && typeof ui.setObjective === 'function') ui.setObjective(key);
  else { const el = ensureObjectiveEl(); if (el) el.textContent = key; }
}
// targets(zoneId?): active contract targets for the board/minimap — [{id, title, zone, type, x, z, cell}].
export function targets(zoneId) {
  if (!ctx) return [];
  const out = [];
  for (const id of store().active) {
    const c = CONTRACTS[id]; if (!c || (zoneId && c.zone !== zoneId)) continue;
    const meta = ZONES[c.zone], d = c.spot != null && meta && meta.spots[c.spot];
    out.push({ id, title: c.title, zone: c.zone, type: c.type, cell: d ? d.cell.slice() : null, x: d ? d.cell[0] + 0.5 : null, z: d ? d.cell[1] + 0.5 : null, progress: progress(id), goal: goalOf(c) });
  }
  return out;
}
// list(): every contract with its status (debug/board).
export function list() { return CONTRACT_ORDER.map(id => ({ id, title: CONTRACTS[id].title, poster: CONTRACTS[id].poster, zone: CONTRACTS[id].zone, type: CONTRACTS[id].type, status: status(id), progress: progress(id), goal: goalOf(CONTRACTS[id]) })); }

/* ============================================================
   Event handlers
   ============================================================ */
function onLantern({ x, z }) {
  const s = store();
  for (const id of s.active.slice()) {
    const c = CONTRACTS[id];
    if (!c || c.type !== 'plant' || !inZone(c)) continue;
    const p = spotPos(id); if (!p) continue;
    if (dist2d(x, z, p.x, p.z) <= CONTRACT_CFG.plantR) complete(id);
  }
}
function onPickup({ kind, item }) {
  if (kind === 'quest') {
    const id = item && item.questId, c = id && CONTRACTS[id];
    if (c) { toast(`You found the ${c.item}. Bring it to the ${ctx.zone.map && ctx.zone.map.stairs && ctx.zone.map.stairs.kind === 'elevator' ? 'cage' : 'stairs'}.`); setProgress(id, 1); }
  } else if (kind === 'bundle' && item && item.contents && (item.contents.quest | 0) > 0) {
    for (const id of store().active) { const c = CONTRACTS[id]; if (c && c.type === 'recover' && inZone(c)) setProgress(id, 1); }
  }
  refreshHud();
}
function onBank({ carried, zoneId }) {
  const s = store();
  for (const id of s.active.slice()) {
    const c = CONTRACTS[id];
    if (!c || c.zone !== zoneId) continue;
    if (c.type === 'fetch') {
      const have = (carried && carried[c.kind]) | 0;
      if (have >= c.n) complete(id);
      else if (have > 0) { s.progress[id] = 0; ctx.events.emit('contractFailed', { id, reason: 'short', progress: have, goal: c.n }); toast(`${c.title}: ${have}/${c.n} — not enough in one run.`); }
    } else if (c.type === 'recover') {
      if (((carried && carried.quest) | 0) > 0) complete(id);
    }
  }
  resetRun('bank', zoneId);
}
function onDeath() { resetRun('death'); }
function onZoneEnter({ zoneId }) {
  resetRun('enter', zoneId);
  spawnQuestsFor(zoneId);
  refreshHud(true);
}
function onZoneExit({ zoneId }) { resetRun('exit', zoneId); }
function onRescued({ id }) {
  const o = available(id);
  if (o) toast(`${o.title} — a contract from the Lantern. Talk to them at the flame.`);
}

/* ============================================================
   Module API
   ============================================================ */
export function init(c) {
  ctx = c;
  store();
  const ev = ctx.events;
  ev.on('lantern', onLantern);
  ev.on('lanternPlanted', onLantern);
  ev.on('pickup', onPickup);
  ev.on('bank', onBank);
  ev.on('death', onDeath);
  ev.on('zoneEnter', onZoneEnter);
  ev.on('zoneExit', onZoneExit);
  ev.on('npcRescued', onRescued);
  ev.on('flameTier', ({ initial }) => { if (initial) { store(); refreshHud(true); } });   // save reset re-emits the initial tier
  // Coordination surface for npc.js (ctx.contracts.available / accept) and the debug API.
  const api = { CONTRACTS, CONTRACT_ORDER, CONTRACT_CFG, MAX_ACTIVE, available, accept, abandon, active, done, status, progress, hudLines, targets, list, get, spotPos, objectiveText, rewardText };
  if (!ctx.contracts) ctx.contracts = api; else Object.assign(ctx.contracts, api);
  if (typeof window !== 'undefined') setTimeout(() => { if (window.__game && !window.__game.contracts) window.__game.contracts = ctx.contracts; }, 0);
  refreshHud(true);
}
export function update(c, dt) {
  if (!ctx) return;
  const st = ctx.state;
  if (st.mode === 'ZONE' && !st.paused && ctx.zone && ctx.zone.id) {
    const s = store(), p = ctx.player;
    for (const id of s.active.slice()) {
      const cdef = CONTRACTS[id];
      if (!cdef || cdef.zone !== ctx.zone.id) continue;
      if (cdef.type === 'fetch') setProgress(id, (p.carried[cdef.kind] | 0));
      else if (cdef.type === 'recover') setProgress(id, (p.carried.quest | 0) > 0 ? 1 : 0);
      else if (cdef.type === 'survive') {
        const sp = spotPos(id); if (!sp) continue;
        const near = dist2d(p.x, p.z, sp.x, sp.z) <= CONTRACT_CFG.spotR, lampOk = !cdef.lampOff || !p.lampOn;
        if (near && lampOk) {
          const v = (s.progress[id] || 0) + dt;
          s.progress[id] = v;
          if (v >= cdef.seconds) complete(id);
        } else if ((s.progress[id] || 0) > 0) s.progress[id] = 0;
      }
    }
  }
  refreshHud();
}
