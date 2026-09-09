// Drive the Three.js prototype headlessly for parity screenshots: node proto.mjs "<steps>"
// Steps are `;`-separated: `wait N` (GAME seconds, read off __game.game.time — headless SwiftShader
// runs the prototype at a few fps, so wall-clock waits would be ~10x short), `shot <path.png>`, or
// any JS expression evaluated in the page (window.__game available).
import { chromium } from 'playwright-core';
const [steps] = process.argv.slice(2);
const browser = await chromium.launch({ executablePath: '/usr/bin/chromium', headless: true,
  args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
page.on('console', m => { if (m.type() === 'error') console.error('page:', m.text()); });
await page.goto('http://127.0.0.1:8765/prototype/index.html', { waitUntil: 'load' });
await page.waitForFunction(() => window.__game && window.__game.actions, null, { timeout: 30000 });
await page.mouse.click(10, 10);
const short = v => { let s; try { s = JSON.stringify(v); } catch { s = String(v); } return s && s.length > 160 ? s.slice(0, 160) + '…' : s; };
for (const raw of steps.split(';')) {
  const s = raw.trim(); if (!s) continue;
  const w = /^wait\s+([\d.]+)$/.exec(s);
  if (w) { const n = Number(w[1]);
    await page.waitForFunction(t0 => window.__game.game.time >= t0, await page.evaluate(() => window.__game.game.time) + n, { timeout: 120000, polling: 100 });
    continue; }
  const sh = /^shot\s+(\S+)$/.exec(s);
  if (sh) { await page.screenshot({ path: sh[1] });
    console.log(`shot ${sh[1]} mode=${await page.evaluate(() => window.__game.game.mode)} t=${(await page.evaluate(() => window.__game.game.time)).toFixed(2)} player=${short(await page.evaluate(() => ({x:+window.__game.player.x.toFixed(2),z:+window.__game.player.z.toFixed(2),yaw:+window.__game.player.yaw.toFixed(2)})))}`);
    continue; }
  const r = await page.evaluate(`(()=>{const v=(${s}); try{return JSON.parse(JSON.stringify(v));}catch(e){return String(v);}})()`);
  console.log(`${s} -> ${short(r)}`);
}
await browser.close();
