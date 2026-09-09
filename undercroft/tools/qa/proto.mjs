// Drive the Three.js prototype headlessly: node proto.mjs "<js steps separated by ;>" out.png
// Each step is either `wait N` (seconds) or a JS expression evaluated in the page (window.__game available).
import { chromium } from 'playwright-core';
const [steps, out] = process.argv.slice(2);
const browser = await chromium.launch({ executablePath: '/usr/bin/chromium', headless: true,
  args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
page.on('console', m => { if (m.type() === 'error') console.error('page:', m.text()); });
await page.goto('http://127.0.0.1:8765/prototype/index.html', { waitUntil: 'load' });
await page.waitForFunction(() => window.__game && window.__game.actions, null, { timeout: 30000 });
// the audio context wants a gesture; a click also starts things the way a player would
await page.mouse.click(10, 10);
for (const raw of steps.split(';')) {
  const s = raw.trim(); if (!s) continue;
  const w = /^wait\s+([\d.]+)$/.exec(s);
  if (w) { await page.waitForTimeout(Number(w[1]) * 1000); continue; }
  const r = await page.evaluate(s);
  console.log(`${s} -> ${JSON.stringify(r)}`);
}
await page.screenshot({ path: out });
console.log('mode', await page.evaluate(() => window.__game.game.mode), 'player', await page.evaluate(() => ({x: window.__game.player.x, z: window.__game.player.z, yaw: window.__game.player.yaw})));
await browser.close();
