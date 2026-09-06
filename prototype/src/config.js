// config.js — every tunable number in one place (DESIGN.md §5–7, DESIGN-v2.md §10).
// Pure data: no imports, no side effects. Any module may import this file.

export const HUB_OX = 60;                       // hub grid x offset in world units
export const SAVE_KEY = 'undercroft-v2';         // v2 save key (DESIGN-v2 §8)
export const SAVE_KEY_V1 = 'undercroft-proto';   // v1 key: imported once, and still mirrored for compatibility

export const CFG = {
  walk: 3.5, sprint: 6.0, radius: 0.3, eye: 1.6, fov: 75, fovSprint: 82, lookKeys: 1.9, mouseSens: 0.002,
  oilMax: 100, startOil: [50, 60, 70, 80], burn: 0.5, deepBurnMul: 1.5, lowOil: 20,
  lampColor: 0xffb265, lampInt: 9.0, lampDist: 16, deepLampMul: 0.6,
  flashCost: 15, flashCd: 1.5, flashDur: 0.15, flashMul: 8, flashRange: 7, flashDot: 0.4, flashFx: 0.1,
  lanternCost: 20, lanternCd: 1.0, lanternMax: 4, poolR: 2.5, flaskOil: 25,
  interactR: 1.6, fadeT: 0.3, toastT: 2.5, dyingT: 1.2,
  // v2 (DESIGN-v2 §2/§10): water cells
  waterWalkMul: 0.55, waterSprintMul: 0.6, hunterWaterMul: 0.85,
  // v2 (DESIGN-v2 §2): Source deep bands
  bandBurn: (lap) => 1.1 + 0.12 * lap, bandLamp: (lap) => 0.95 - 0.08 * lap,
};

// Shared hunter senses/timers (DESIGN.md §6) — identical for every profile.
export const HUNTER = {
  tick: 0.2, repath: 0.3, lampR: 12, sprintR: 9, walkR: 2.5, stillR: 1.0, waterR: 7,
  staggerT: 3, dazeT: 4, unreachT: 6, waitT: 2, wanderCells: 10, farCells: 12,
  followerLitR: 3, catchBusyT: 2,
};
// Per-profile speeds/eyes (DESIGN-v2 §2: `base` = v1, `fast` = Ossuary/Source).
export const HUNTER_PROFILES = {
  base: {
    speed: { WANDER: 1.8, INVESTIGATE: 3.0, CHASE: 4.3, STAGGERED: 0 },
    eye: { WANDER: 0.4, INVESTIGATE: 0.8, CHASE: 1.5, STAGGERED: 0.05 },
    catchR: 0.8, loseT: 3, scaleY: 1.0, eyeColor: 0xff3a20,
  },
  fast: {
    speed: { WANDER: 2.2, INVESTIGATE: 3.6, CHASE: 5.0, STAGGERED: 0 },
    eye: { WANDER: 0.4, INVESTIGATE: 0.8, CHASE: 1.5, STAGGERED: 0.05 },
    catchR: 0.9, loseT: 4, scaleY: 1.15, eyeColor: 0xffa020,
  },
};

export const TIERS = [
  { pts: 0,  int: 2.0, dist: 7,  msg: 'Only embers remain.' },
  { pts: 6,  int: 3.5, dist: 11, msg: 'The flame stirs. The alcoves take shape.' },
  { pts: 15, int: 5.5, dist: 16, msg: 'The flame grows. The vault is warm again.' },
  { pts: 30, int: 8.0, dist: 24, msg: 'The Last Lantern burns bright.' },
];
export const SCONCE_INT = [0.45, 1.6, 3.2, 5.0]; // hub alcove sconce intensity per tier; [0] = the banked-ember glow a sconce keeps below its own tier

export const POINTS = { oil: 1, relic: 3, rich: 5, quest: 0 };
export const LABEL = { oil: 'oil flask', relic: 'relic', rich: 'rich relic', bundle: 'your lost bundle', quest: 'quest item' };

// DESIGN-v2 §7 — Workshop light-tech tiers (index 0 = none).
export const LIGHT_TECH = [
  { distMul: 1.0,  burnMul: 1.0, flashCost: 15, lanternCost: 20, cost: { relics: 0,  rich: 0 } },
  { distMul: 1.15, burnMul: 0.9, flashCost: 15, lanternCost: 20, cost: { relics: 6,  rich: 0 } },
  { distMul: 1.3,  burnMul: 0.8, flashCost: 12, lanternCost: 20, cost: { relics: 10, rich: 0 } },
  { distMul: 1.45, burnMul: 0.7, flashCost: 10, lanternCost: 16, cost: { relics: 14, rich: 2 } },
];
// DESIGN-v2 §7 — building costs and gates.
export const BUILD_COSTS = {
  workshop: { relics: 6, npc: 'lamplighter' },
  press:    { oil: 80, npc: 'keeper' },
  cart:     { oil: 100, npc: 'cartographer' },
  shrine:   { oil: 150, npc: 'deacon' },
  tram:     { oil: 120, tier: 2 },
  elevator: { oil: 250, relics: 4, tier: 3 },
  pressRelicOil: 30, reservoir: [5, 8], reservoirOil: 15, blessingOil: 40,
};
// DESIGN-v2 §1/§10 — audio master numbers.
export const AUDIO = { vol: 0.8, volStep: 0.1, presenceMax: 0.22, presenceRange: 18, stingGap: 6,
  droneHub: 0.03, droneZone: 0.08, crossfade: 1.5 };
// Key bindings (KeyboardEvent.code).
export const KEYS = {
  forward: 'KeyW', back: 'KeyS', left: 'KeyA', right: 'KeyD', sprint: ['ShiftLeft', 'ShiftRight'],
  lookLeft: 'ArrowLeft', lookRight: 'ArrowRight', lookUp: 'ArrowUp', lookDown: 'ArrowDown',
  lamp: 'KeyF', flash: 'KeyQ', lantern: 'KeyR', interact: 'KeyE', topUp: 'KeyT',
  mute: 'KeyM', volDown: 'BracketLeft', volUp: 'BracketRight', minimap: 'Tab', reset: 'Backspace',
  confirm: ['Enter', 'Space'], menuClose: 'Escape', menuPick: ['Digit1', 'Digit2', 'Digit3', 'Digit4'],
};
// Tool display names (gates: "Locked — needs Pry Bar").
export const TOOLS = { prybar: 'Pry Bar', sluice: 'Sluice Key', censer: 'Censer' };
// Hub warmth (DESIGN.md §3 hub): ambient / fog tint per flame tier — the hub only; zones keep their PALETTES.
export const HUB_WARMTH = {
  ambient: [0x1c140d, 0x2a1d12, 0x3a2917, 0x4c3620],
  fog: { color: [0x050302, 0x080504, 0x0b0705, 0x0f0a06], density: 0.10 },
  lanternGlow: 1.0,          // hanging-lantern glass brightness multiplier (per-tier merged groups)
  lanternLight: { color: 0xffb060, int: 1.2, dist: 5.5 },   // the five keyed lanterns carry a PointLight (culled with the hub in zones)
};
// Great-flame embers: a Points cloud rising from the brazier; `count` alive per tier.
export const EMBERS = { max: 90, perTier: [18, 36, 60, 90], rise: [0.5, 1.3], life: [1.4, 2.8], drift: 0.35, size: 0.22 };
// Hub collision mask bits (hub.js/world.js write map.blockMask; world.moveWithCollision treats any bit as solid).
export const HUB_BLOCK = { PROP: 1, BUILDING: 2, NPC: 4, FLAME: 8, minTop: 0.25, maxBottom: 1.2, shrink: 0.2, minSize: 0.25 };   // minSize: posts/legs/candles thinner than this in both axes never block on their own
