// ui.js — every DOM read/write: HUD, hint, toast, screens, fade/vignette/flash overlays, menus (DESIGN.md §8).
// Also the two list menus: the main menu (#title, replaces the old click-to-start screen) and the pause menu
// (#pausemenu), both driven by one keyboard/mouse list component with Controls / Sound / confirm sub-panels.
// Listens: hunterState (CHASE/POUNCE/SURGE → "It has seen you"), flameTier (toast), toast, flash, death, hubEnter, begin.
// The hint line also appends ctx.hunter.hint() — the per-creature HUD row (DESIGN.md §5.1–5).
import { CFG, TIERS, LABEL } from './config.js';

export const VERSION = 'prototype v2';
let ctx = null;
const ui = { seenT: 0, flashFx: 0, toastQ: [], toastT: 0, textCache: {}, contractLines: [] };

export function init(c) {
  ctx = c;
  const dom = ctx.dom;
  for (const id of ['hud', 'oilfill', 'oilnum', 'lamp', 'sound', 'cdq', 'cdr', 'carried', 'flamehud', 'contracts', 'hint', 'pause',
    'vignette', 'flashfx', 'fade', 'toast', 'title', 'death', 'deathmsg', 'deathnote',
    'menu', 'menutitle', 'menubody', 'menufoot', 'minimap',
    'mmsub', 'mmtext', 'mmtable', 'mmlist', 'mmfoot', 'pausemenu', 'pmtitle', 'pmtext', 'pmtable', 'pmlist', 'pmfoot']) dom[id] = document.getElementById(id);
  dom.death.addEventListener('click', () => ctx.actions.returnToHub());
  mainMenu = makeListMenu({ root: dom.title, text: dom.mmtext, table: dom.mmtable, list: dom.mmlist, foot: dom.mmfoot, sub: dom.mmsub });
  pauseMenu = makeListMenu({ root: dom.pausemenu, title: dom.pmtitle, text: dom.pmtext, table: dom.pmtable, list: dom.pmlist, foot: dom.pmfoot });
  const ev = ctx.events;
  // "It has seen you": a base/fast/Brute CHASE, a false light's POUNCE and a Drowner's SURGE (DESIGN.md §5.3–5)
  ev.on('hunterState', ({ state, prev }) => { if (state !== prev && (state === 'CHASE' || state === 'POUNCE' || state === 'SURGE')) ui.seenT = 2; });
  ev.on('flameTier', ({ tier, initial }) => { if (!initial) toast(TIERS[tier - 1].msg); });
  ev.on('toast', ({ msg }) => toast(msg));
  ev.on('flash', () => { ui.flashFx = CFG.flashFx; });
  ev.on('death', () => dom.vignette.classList.add('red'));
  ev.on('hubEnter', () => dom.vignette.classList.remove('red'));
  ev.on('title', flushToasts);         // nothing from the run plays out over the main menu
  ev.on('saveReset', flushToasts);     // 'Save wiped.' shows at once, not after a backlog
}

/* ---------- toast / hint ---------- */
// While a list menu is open the Sound panel shows the level itself: its volume/mute toasts are dropped rather than
// queued up to replay, stale, once the game resumes.
const menuOpen = () => !!((mainMenu && mainMenu.isOpen()) || (pauseMenu && pauseMenu.isOpen()));
export function toast(msg) { if (menuOpen() && /^(Volume \d+%|Sound (on|off))$/.test(msg)) return; ui.toastQ.push(msg); }
export function flushToasts() { ui.toastQ.length = 0; ui.toastT = 0; const el = ctx.dom.toast; el.textContent = ''; el.style.opacity = '0'; }
function updateToast(dt) {
  const el = ctx.dom.toast;
  if (ui.toastT > 0) { ui.toastT -= dt; if (ui.toastT <= 0) el.style.opacity = '0'; else return; }
  if (ui.toastT <= -0.35 || (ui.toastT <= 0 && !el.textContent)) {
    if (ui.toastQ.length) { el.textContent = ui.toastQ.shift(); el.style.opacity = '1'; ui.toastT = CFG.toastT; }
  } else if (ui.toastT <= 0) ui.toastT -= dt; // short gap between toasts
}
const NOT_SAFE = 'Not safe — it will wade in';   // hunter.hint()'s Brute line: it replaces "Safe — it will not enter the light"
// hint(text): force the bottom hint line ('' clears the override).
export function hint(text) { ctx.state.hintOverride = text || ''; }
function setText(key, s) { if (ui.textCache[key] !== s) { ui.textCache[key] = s; ctx.dom[key].textContent = s; } }

function targetLabel(t) {
  if (!t) return '';
  if (t.label) return t.label;
  if (t.type === 'item') return `[E] Pick up ${LABEL[t.item.kind] || t.item.kind}`;
  if (t.type === 'bank') return t.empty ? '[E] Return to the Lantern' : '[E] Bank loot';
  if (t.type === 'descend') return '[E] Descend';
  if (t.type === 'gate') return t.locked ? `Locked — needs ${t.toolName}` : '[E] Open the gate';
  if (t.type === 'shortcut') return t.barred ? 'Barred from the other side' : '[E] Lift the bars';
  return '';
}
export function hintText() {
  const st = ctx.state, p = ctx.player;
  if (st.hintOverride) return st.hintOverride;
  if (st.mode === 'HUB') return targetLabel(ctx.actions.interactTarget());
  if (st.mode !== 'ZONE') return '';
  // alarm · action prompt · lamp/safety status — shown together so none hides another
  const parts = [];
  if (ui.seenT > 0) parts.push('It has seen you');
  // the creature line (DESIGN.md §5.1–5 HUD rows): '' when none applies; hunter.js owns which one
  const creature = ctx.hunter && typeof ctx.hunter.hint === 'function' ? ctx.hunter.hint() : '';
  if (creature && !parts.includes(creature)) parts.push(creature);
  const label = targetLabel(ctx.actions.interactTarget());
  if (label) parts.push(label);
  if (p.oil <= 0) parts.push(p.carried.oil ? 'The lamp is dry — [T] pour a flask' : 'The lamp is dry');
  else if (p.lampOn && p.oil < CFG.lowOil) parts.push('Lamp guttering');
  else if (p.inPool && creature !== NOT_SAFE) parts.push('Safe — it will not enter the light');   // a Brute in reach replaces the Safe line
  return parts.join('   ·   ');
}

/* ---------- HUD ---------- */
function updateHUD() {
  const dom = ctx.dom, st = ctx.state, p = ctx.player, save = ctx.save, tier = ctx.hub.flame.tier;
  const oil = Math.max(0, Math.min(CFG.oilMax, p.oil));
  dom.oilfill.style.width = `${oil}%`;
  dom.oilfill.classList.toggle('low', oil < CFG.lowOil);
  setText('oilnum', `${Math.ceil(oil)} / ${CFG.oilMax} oil`);
  setText('lamp', st.mode === 'HUB' ? 'LAMP OFF (safe)' : (p.lampOn ? 'LAMP ON' : 'LAMP OFF'));
  setText('sound', ctx.audio && ctx.audio.muted ? '  SOUND OFF' : '');
  dom.cdq.classList.toggle('off', p.flashCd > 0 || p.oil < CFG.flashCost);
  dom.cdr.classList.toggle('off', p.lanternCd > 0 || p.oil < CFG.lanternCost);
  const c = p.carried;
  setText('carried', `Carried: ${c.oil || c.relic || c.rich ? `${c.oil} flask · ${c.relic} relic · ${c.rich} rich` : 'nothing'}`);
  const next = TIERS[tier] ? `${save.points}/${TIERS[tier].pts}` : `${save.points} pts`;
  setText('flamehud', `Flame: tier ${tier} (${next})`);
  setText('contracts', ui.contractLines.join('\n'));
  setText('hint', hintText());
  dom.pause.hidden = !(st.paused || st.mouseFree);
  // vignette: darkens as oil runs low; red while dying/dead
  let v = 0;
  if (st.mode === 'ZONE' && p.oil < CFG.lowOil) v = 0.75 * (CFG.lowOil - p.oil) / CFG.lowOil;
  if (st.mode === 'DYING' || st.mode === 'DEAD') v = 0.9;
  dom.vignette.style.opacity = v.toFixed(3);
  dom.flashfx.style.opacity = ui.flashFx > 0 ? Math.min(1, ui.flashFx / CFG.flashFx).toFixed(3) : '0';
}
export function update(c, dt) {
  ui.seenT = Math.max(0, ui.seenT - dt);
  ui.flashFx = Math.max(0, ui.flashFx - dt);
  updateToast(dt);
  updateHUD();
}
export function setFade(a) { ctx.dom.fade.style.opacity = a.toFixed(3); }
// setContractLines(lines): HUD lines under "Flame:" (one per active contract, DESIGN-v2 §4).
export function setContractLines(lines) { ui.contractLines = lines || []; }

/* ---------- screens ---------- */
export function showTitle() { ctx.dom.hud.hidden = true; refreshMainMenu(); mainMenu.open(mainRoot()); }
export function hideTitle() { mainMenu.close(); ctx.dom.hud.hidden = false; }
export function showDeath(lostLoot, lostAny) {
  const dom = ctx.dom;
  dom.deathmsg.textContent = `Lost: ${lostLoot}.`;
  dom.deathnote.textContent = lostAny
    ? 'Your bundle lies where you fell. The flame at the Lantern is untouched.'
    : 'You carried nothing down. The flame at the Lantern is untouched.';
  dom.death.hidden = false; dom.hud.hidden = true;
}
export function hideDeath() { ctx.dom.death.hidden = true; ctx.dom.hud.hidden = false; }

// Generic menu panel (#menu): used by the dialog/board/build/ending screens. The caller (main) owns the
// MENU/ENDING mode switch; this only draws. lines: string[] (a line starting with '  ' is dimmed).
export function showMenu({ title = '', lines = [], foot = 'Esc to close' } = {}) {
  const dom = ctx.dom;
  dom.menutitle.textContent = title;
  dom.menubody.replaceChildren(...lines.map(l => { const d = document.createElement('div'); d.textContent = l; if (l.startsWith('  ')) d.className = 'dim'; return d; }));
  dom.menufoot.textContent = foot;
  dom.menu.hidden = false;
}
export function closeMenu() { ctx.dom.menu.hidden = true; }
// TODO owned by npc agent: dialogue layout (line + contract offer/turn-in).
export function showDialog(opts) { return showMenu(opts); }
// TODO owned by hub agent: departure board (zones + lock reasons, 1–4 selects).
export function showBoard(opts) { return showMenu(opts); }
// TODO owned by hub agent: build/service menu.
export function showBuild(opts) { return showMenu(opts); }
// TODO owned by endgame agent: ending screen (title, 4 lines, stats, click to continue).
export function showEnding(opts) { return showMenu({ foot: 'Click to continue', ...opts }); }
// TODO owned by hub agent: minimap canvas (200×200, 5 px/cell) — Tab toggles #minimap.
export function toggleMinimap() { const m = ctx.dom.minimap; if (m) m.hidden = !m.hidden; return m ? !m.hidden : false; }

/* ============================================================
   List menus — one component for the main menu and the pause menu.
   A menu holds a stack of panels. panel = { title?, text?, table?, items(): item[], foot?, onEscape?() }
   item = { label, note?, disabled?, run?(), adjust?(dir), key? }. Keyboard: ↑/↓ or W/S move, ←/→ or [ ] adjust,
   Enter / Space / E activate, 1–9 pick by number, Esc pops a sub-panel (or panel.onEscape at the root).
   Mouse: hover selects, click activates. The caller (main.js) owns the mode switches; this only draws and routes.
   ============================================================ */
let mainMenu = null, pauseMenu = null;
const KEY_UP = ['ArrowUp', 'KeyW'], KEY_DOWN = ['ArrowDown', 'KeyS'], KEY_LEFT = ['ArrowLeft', 'BracketLeft'], KEY_RIGHT = ['ArrowRight', 'BracketRight'];
const KEY_GO = ['Enter', 'Space', 'KeyE', 'NumpadEnter'];

function makeListMenu(el) {
  const m = { el, stack: [], items: [], sel: 0, openedAt: 0 };
  const cur = () => m.stack[m.stack.length - 1] || null;
  const emit = (n) => { if (ctx && ctx.events) ctx.events.emit(n, { menu: el.root.id }); };
  m.isOpen = () => !el.root.hidden;
  m.depth = () => m.stack.length;
  m.panel = cur;
  m.render = function () {
    const p = cur(); if (!p) return;
    m.items = (typeof p.items === 'function' ? p.items() : p.items) || [];
    if (m.sel >= m.items.length) m.sel = Math.max(0, m.items.length - 1);
    if (m.items[m.sel] && m.items[m.sel].disabled) { const i = m.items.findIndex(it => !it.disabled); if (i >= 0) m.sel = i; }
    if (el.title) el.title.textContent = p.title || '';
    if (el.sub) { el.sub.textContent = p.sub || ''; el.sub.hidden = !p.sub; }
    el.text.replaceChildren(...(p.text ? [].concat(p.text).map(t => { const d = document.createElement('p'); d.textContent = t; return d; }) : []));
    el.text.hidden = !p.text;
    if (p.table) {
      const tb = document.createElement('table');
      for (const [k, v] of p.table) { const tr = document.createElement('tr'); const a = document.createElement('td'); a.textContent = k; const b = document.createElement('td'); b.textContent = v; tr.append(a, b); tb.appendChild(tr); }
      el.table.replaceChildren(tb); el.table.hidden = false;
    } else { el.table.replaceChildren(); el.table.hidden = true; }
    el.list.replaceChildren(...m.items.map((it, i) => {
      const d = document.createElement('div');
      d.className = 'mi' + (i === m.sel ? ' sel' : '') + (it.disabled ? ' dis' : '') + (it.danger ? ' danger' : '');
      d.dataset.i = String(i);
      const k = document.createElement('span'); k.className = 'mk'; k.textContent = it.key || String(i + 1);
      const l = document.createElement('span'); l.className = 'ml'; l.textContent = it.label;
      d.append(k, l);
      if (it.note) { const n = document.createElement('span'); n.className = 'mn'; n.textContent = it.note; d.appendChild(n); }
      d.addEventListener('click', (e) => { e.stopPropagation(); m.activate(i); });
      return d;
    }));
    el.foot.textContent = p.foot != null ? p.foot : '';
    el.foot.hidden = !el.foot.textContent;
  };
  // Hover-select rides on a real mouse move only. The rows are rebuilt on every render, and Chromium then sends a
  // synthetic mouseenter/mousemove to whatever fresh row sits under a resting cursor (pointer-lock release puts it back
  // over the list): honouring that undid every keyboard move within a frame. So a move must change clientX/Y.
  m.mx = -1; m.my = -1;
  el.list.addEventListener('mousemove', (e) => {
    if (e.clientX === m.mx && e.clientY === m.my) return;
    m.mx = e.clientX; m.my = e.clientY;
    const row = e.target && e.target.closest ? e.target.closest('.mi') : null; if (!row) return;
    const i = +row.dataset.i, it = m.items[i];
    if (it && !it.disabled && m.sel !== i) { m.sel = i; m.render(); }
  });
  m.open = function (panel) { m.stack = [panel]; m.sel = panel.sel | 0; m.openedAt = performance.now(); el.root.hidden = false; m.render(); };
  m.push = function (panel) { if (cur()) cur().sel = m.sel; m.stack.push(panel); m.sel = panel.sel | 0; m.render(); };
  m.pop = function () { if (m.stack.length <= 1) return false; m.stack.pop(); m.sel = cur().sel | 0; m.render(); emit('menuBack'); return true; };
  m.close = function () { el.root.hidden = true; m.stack = []; m.items = []; };
  m.move = function (dir) {
    if (!m.items.length) return;
    let i = m.sel;
    for (let n = 0; n < m.items.length; n++) { i = (i + dir + m.items.length) % m.items.length; if (!m.items[i].disabled) break; }
    if (i !== m.sel) { m.sel = i; m.render(); emit('menuMove'); }
  };
  m.activate = function (i) {
    const it = m.items[i]; if (!it || it.disabled) { if (it) emit('uiError'); return false; }
    m.sel = i;
    emit('menuSelect');
    if (typeof it.run === 'function') it.run(it);
    else if (typeof it.adjust === 'function') it.adjust(+1);
    if (m.isOpen() && cur()) m.render();
    return true;
  };
  m.adjust = function (dir) {
    const it = m.items[m.sel]; if (!it || typeof it.adjust !== 'function') return false;
    it.adjust(dir); emit('menuMove'); m.render(); return true;
  };
  // key(code) → true when consumed
  m.key = function (code) {
    if (!m.isOpen() || !cur()) return false;
    if (KEY_UP.includes(code)) { m.move(-1); return true; }
    if (KEY_DOWN.includes(code)) { m.move(+1); return true; }
    if (KEY_LEFT.includes(code)) return m.adjust(-1);
    if (KEY_RIGHT.includes(code)) return m.adjust(+1);
    if (KEY_GO.includes(code)) { m.activate(m.sel); return true; }
    const n = /^(?:Digit|Numpad)([1-9])$/.exec(code);
    if (n) { const i = parseInt(n[1], 10) - 1; if (i < m.items.length) m.activate(i); else emit('uiError'); return true; }
    if (code === 'Escape') {
      if (m.stack.length > 1) { m.pop(); return true; }
      const p = cur(); if (typeof p.onEscape === 'function') { p.onEscape(); return true; }
    }
    return false;
  };
  return m;
}

/* ---------- shared sub-panels ---------- */
export const CONTROLS = [
  ['Mouse', 'Look (click to lock the pointer)'],
  ['Arrow keys', 'Look without the mouse · move the menu selection'],
  ['W A S D', 'Move'],
  ['Shift', 'Sprint — faster, but heard 9 u away'],
  ['F', 'Handlamp on / off (dark = stealth, burns nothing)'],
  ['Q', 'Flash — spend oil to stagger the hunter in front of you'],
  ['R', 'Plant lantern — spend oil for a pool of light it will not enter'],
  ['E', 'Interact: pick up · free · open gate · bank · talk · build · descend · confirm'],
  ['T', 'Pour a carried flask into the lamp (+25 oil)'],
  ['Tab', "Minimap (needs the Cartographer's Table)"],
  ['M', 'Mute'],
  ['[  ]', 'Volume down / up'],
  ['1 – 5', 'Pick a line in any menu'],
  ['Enter / Space', 'Confirm'],
  ['Esc', 'Pause menu · close a menu · back'],
  ['Backspace ×2', 'On the main menu: wipe the save (shortcut)'],
];
const backItem = (menu) => ({ label: 'Back', key: '←', run: () => menu.pop() });
function controlsPanel(menu) {
  return { title: 'CONTROLS', table: CONTROLS, items: () => [backItem(menu)], foot: 'Esc or Enter to go back' };
}
function soundPanel(menu) {
  const a = ctx.audio;
  const bar = () => { const n = Math.round((a.vol || 0) * 10); return '█'.repeat(n) + '░'.repeat(10 - n); };
  return {
    title: 'SOUND',
    items: () => [
      { label: `Volume  ${bar()}  ${Math.round((a.vol || 0) * 100)}%`, key: '◂ ▸', note: '← → or [ ] to change',
        adjust: (dir) => a.setVolume(Math.round(((a.vol || 0) + dir * 0.1) * 10) / 10),
        run: () => a.setVolume(a.vol >= 0.999 ? 0 : Math.round((a.vol + 0.1) * 10) / 10) },
      { label: `Mute: ${a.muted ? 'ON' : 'off'}`, note: 'M toggles in play', run: () => a.toggleMute() },
      backItem(menu),
    ],
    foot: 'All sound is synthesised — nothing to download. Esc to go back',
  };
}
function confirmPanel(menu, { title, text, yes, no = 'No', onYes, danger = true }) {
  return { title, text, items: () => [
    { label: yes, danger, run: () => { onYes(); } },
    { label: no, run: () => menu.pop() },
  ], sel: 1, foot: 'Esc to go back' };
}

/* ---------- main menu ---------- */
function mainRoot() {
  const A = ctx.actions, info = A.saveInfo ? A.saveInfo() : { hasProgress: false, text: '' };
  const items = [];
  if (info.hasProgress) items.push({ label: 'Continue', note: info.text, run: () => A.begin() });
  items.push({ label: 'New Game', note: info.hasProgress ? 'starts over' : 'the Lantern is nearly out', run: () => A.newGame() });
  items.push({ label: 'Controls', run: () => mainMenu.push(controlsPanel(mainMenu)) });
  items.push({ label: 'Sound', run: () => mainMenu.push(soundPanel(mainMenu)) });
  return { title: '', items: () => items, foot: `The Undercroft — ${VERSION} · three.js r160 · voxel models and procedural audio, no assets · ↑↓ Enter or click`, onEscape: () => {} };
}
// refreshMainMenu(): redraw the root list (Continue appears/disappears with the save).
export function refreshMainMenu() {
  if (!mainMenu || !mainMenu.isOpen()) return;
  const keepSel = mainMenu.depth() === 1 ? mainMenu.sel : 0;
  mainMenu.open({ ...mainRoot(), sel: keepSel });
}
// confirmNewGame(): the "This wipes your save" sub-menu; Yes → actions.newGame(true).
export function confirmNewGame() {
  if (!mainMenu || !mainMenu.isOpen()) return false;
  mainMenu.push(confirmPanel(mainMenu, { title: 'NEW GAME', text: 'This wipes your save: the flame, every building, everyone you rescued, every ending seen.',
    yes: 'Yes, start over', no: 'No, keep my save', onYes: () => ctx.actions.newGame(true) }));
  return true;
}
export function mainMenuKey(code) { return mainMenu ? mainMenu.key(code) : false; }
export function mainMenuState() { return mainMenu ? { open: mainMenu.isOpen(), depth: mainMenu.depth(), sel: mainMenu.sel, title: mainMenu.panel() ? mainMenu.panel().title : '', items: mainMenu.items.map(i => i.label) } : null; }

/* ---------- pause menu ---------- */
function pauseRoot(inZone) {
  const A = ctx.actions, carried = ctx.player.carried, has = (carried.oil | 0) + (carried.relic | 0) + (carried.rich | 0) + (carried.quest | 0) > 0;
  const loot = A.describe ? A.describe(carried) : '';
  const items = [
    { label: 'Resume', run: () => A.closePause() },
    { label: 'Controls', run: () => pauseMenu.push(controlsPanel(pauseMenu)) },
    { label: 'Sound', run: () => pauseMenu.push(soundPanel(pauseMenu)) },
    { label: 'Return to main menu', note: inZone ? 'abandons this run' : '', run: () => {
      if (!inZone) { A.toMainMenu(); return; }
      pauseMenu.push(confirmPanel(pauseMenu, { title: 'LEAVE THE DARK?',
        text: has ? `You are still below. Carried loot is LOST if you leave now: ${loot}. Bank it at the stairs to keep it.`
          : 'You are still below. Anything you pick up before banking would be lost — right now you carry nothing.',
        yes: has ? 'Leave — lose the loot' : 'Leave the run', no: 'Stay', onYes: () => A.toMainMenu() }));
    } },
    { label: 'Clear save', note: 'wipes everything', danger: true, run: () => pauseMenu.push(confirmPanel(pauseMenu, { title: 'CLEAR SAVE?',
      text: 'This wipes your save: the flame, every building, everyone you rescued, every ending seen. You return to the main menu.',
      yes: 'Yes, wipe it', no: 'No, keep it', onYes: () => A.clearSave() })) },
  ];
  return { title: 'PAUSED', sub: '', items: () => items, foot: 'Esc resumes · ↑↓ Enter or click · 1–5', onEscape: () => A.closePause() };
}
export function showPause(inZone) { pauseMenu.open(pauseRoot(!!inZone)); }
export function hidePause() { if (pauseMenu) pauseMenu.close(); }
export function pauseKey(code) { return pauseMenu ? pauseMenu.key(code) : false; }
export function pauseState() { return pauseMenu ? { open: pauseMenu.isOpen(), depth: pauseMenu.depth(), sel: pauseMenu.sel, title: pauseMenu.panel() ? pauseMenu.panel().title : '', items: pauseMenu.items.map(i => i.label), openedAt: pauseMenu.openedAt } : null; }
