// save.js — persistent state (DESIGN-v2 §8). `data` is the live object (ctx.save). Key `undercroft-v2`,
// written ≤ 4×/s (debounced 250 ms) on every emitted event and on pagehide. Imports v1 `undercroft-proto`
// (points/bankedOil) once; also keeps mirroring {points, bankedOil} to the v1 key for v1 tooling.
import { SAVE_KEY, SAVE_KEY_V1, AUDIO, TIERS } from './config.js';

export const DEBOUNCE_MS = 250;

function defaults() {
  return {
    v: 2, points: 0, oil: 0, relics: 0, rich: 0, lightTech: 0, reservoir: 0,
    buildings: { workshop: false, press: false, cart: false, shrine: false, tram: false, elevator: false },
    rescued: { lamplighter: false, cartographer: false, keeper: false, deacon: false },
    tools: { prybar: false, sluice: false, censer: false },
    gatesOpened: {},
    contracts: { active: [], done: [], progress: {} },
    zoneSelected: 'undercroft', blessing: false,
    endings: { cage: false, dawn: false, night: false },
    explored: {},
    stats: { runs: 0, deaths: 0, rescues: 0, banked: 0 },
    audio: { vol: AUDIO.vol, muted: false },
    importedV1: false,
  };
}
export const data = defaults();

const isObj = (o) => o && typeof o === 'object' && !Array.isArray(o);
// Merge `src` onto `dst` keeping dst's shape: unknown keys are ignored, wrong types fall back to defaults.
function mergeInto(dst, src) {
  if (!isObj(src)) return dst;
  for (const k of Object.keys(dst)) {
    if (!(k in src)) continue;
    const d = dst[k], s = src[k];
    if (isObj(d)) { if (k === 'gatesOpened' || k === 'explored' || k === 'progress') { if (isObj(s)) dst[k] = { ...s }; } else mergeInto(d, s); }
    else if (Array.isArray(d)) { if (Array.isArray(s)) dst[k] = s.slice(); }
    else if (typeof d === 'number') { if (Number.isFinite(s)) dst[k] = s; }
    else if (typeof d === 'boolean') dst[k] = !!s;
    else if (typeof d === 'string') { if (typeof s === 'string') dst[k] = s; }
  }
  return dst;
}

let timer = 0, dirty = false;
function readKey(key) { try { return JSON.parse(localStorage.getItem(key) || 'null'); } catch (e) { return null; } }

// load(): fill `data` from localStorage (v2), or import the v1 record once. Returns `data`.
export function load() {
  Object.assign(data, defaults());
  const v2 = readKey(SAVE_KEY);
  if (v2 && v2.v === 2) mergeInto(data, v2);
  if (!data.importedV1) {
    const v1 = readKey(SAVE_KEY_V1);
    if (v1 && Number.isFinite(v1.points) && !(v2 && v2.v === 2)) { data.points = Math.max(0, v1.points | 0); data.oil = Math.max(data.oil, v1.bankedOil | 0); }
    data.importedV1 = true;
  }
  data.points = Math.max(0, data.points | 0);
  return data;
}
// flush(): write now (both keys), best effort.
export function flush() {
  dirty = false;
  if (timer) { clearTimeout(timer); timer = 0; }
  try {
    localStorage.setItem(SAVE_KEY, JSON.stringify(data));
    localStorage.setItem(SAVE_KEY_V1, JSON.stringify({ points: data.points, bankedOil: data.oil }));
  } catch (e) { /* ignore: private mode / quota */ }
}
// save(): debounced write.
export function save() {
  dirty = true;
  if (timer) return;
  timer = setTimeout(() => { timer = 0; if (dirty) flush(); }, DEBOUNCE_MS);
}
// reset(): wipe storage and restore defaults in place (ctx.save keeps its identity).
export function reset() {
  try { localStorage.removeItem(SAVE_KEY); localStorage.removeItem(SAVE_KEY_V1); } catch (e) { /* ignore */ }
  if (timer) { clearTimeout(timer); timer = 0; }
  dirty = false;
  Object.assign(data, defaults());
  data.importedV1 = true;
  return data;
}
// hasProgress(): true when the save holds anything worth continuing — the main menu shows "Continue" (and asks
// before "New Game") only then. The v2 key itself is written at boot, so its presence means nothing.
export function hasProgress(d = data) {
  const any = (o) => !!o && Object.values(o).some(Boolean);
  const c = d.contracts || {};
  return (d.stats && (d.stats.runs | 0) > 0) || (d.points | 0) > 0 || (d.oil | 0) > 0 || (d.relics | 0) > 0 || (d.rich | 0) > 0 ||
    (d.lightTech | 0) > 0 || any(d.buildings) || any(d.rescued) || any(d.tools) || any(d.endings) ||
    (Array.isArray(c.active) && c.active.length > 0) || (Array.isArray(c.done) && c.done.length > 0);
}
// summary(): the one-line save summary under "Continue": { tier, rescued, rescuedTotal, endings, endingsTotal, runs, points, text }.
export function summary(d = data) {
  const pts = Math.max(0, d.points | 0);
  let tier = 1; for (let i = 0; i < TIERS.length; i++) if (pts >= TIERS[i].pts) tier = i + 1;
  const count = (o) => (o ? Object.values(o).filter(Boolean).length : 0), total = (o) => (o ? Object.keys(o).length : 0);
  const r = { tier, points: pts, rescued: count(d.rescued), rescuedTotal: total(d.rescued) || 4,
    endings: count(d.endings), endingsTotal: total(d.endings) || 3, runs: d.stats ? d.stats.runs | 0 : 0 };
  r.text = `Flame tier ${r.tier} · ${r.rescued}/${r.rescuedTotal} rescued · ${r.endings}/${r.endingsTotal} endings seen`;
  return r;
}
// init(ctx): save on every event (debounced) and on pagehide.
export function init(ctx) {
  ctx.events.on('*', () => save());
  window.addEventListener('pagehide', flush);
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') flush(); });
}
