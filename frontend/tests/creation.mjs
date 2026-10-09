import { chromium } from '../node_modules/playwright/index.mjs';
import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
const origin = process.env.TAB_TEST_ORIGIN || 'http://127.0.0.1:5197';
mkdirSync('../qa', { recursive: true });
const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, recordVideo: { dir: '../qa/creation-recording', size: { width: 1440, height: 1000 } } });
const page = await context.newPage();
const errors = [];
const writes = [];
page.on('pageerror', (e) => errors.push(e.message));
page.on('request', (req) => { if (req.url().includes('/api/') && !['GET', 'HEAD'].includes(req.method())) writes.push(req.url()); });
await page.goto(origin, { waitUntil: 'networkidle' });
await page.locator('.creation-preview').scrollIntoViewIfNeeded();
await page.waitForTimeout(1300);
await page.getByRole('button', { name: 'Restart agent walkthrough' }).click();
await page.getByLabel('Example agent name', { exact: true }).waitFor();
await page.waitForFunction(() => document.querySelector('[aria-label="Example agent name"]').value === 'wren', null, { timeout: 8000 });
assert.equal(await page.getByLabel('Example agent name', { exact: true }).inputValue(), 'wren');
await page.getByRole('button', { name: 'Pause agent walkthrough' }).click();
const progress = await page.locator('.creation-progress i').getAttribute('style');
await page.waitForTimeout(400);
assert.equal(await page.locator('.creation-progress i').getAttribute('style'), progress, 'pause must stop playback');
await page.getByRole('button', { name: 'Play agent walkthrough' }).click();
await page.locator('.creation-save.saved').waitFor({ timeout: 15000 });
assert.equal(await page.getByLabel('Example daily spending cap', { exact: true }).inputValue(), '25');
assert.equal(await page.locator('.creation-preview input[type=checkbox]:checked').count(), 2);
assert.equal(await page.getByLabel('Example agent purpose', { exact: true }).inputValue(), 'onchain research and daily summaries');
assert.deepEqual(writes, [], 'walkthrough must never write agent plans');
await page.screenshot({ path: '../qa/agent-creation-desktop.png' });
await page.getByRole('button', { name: 'Restart agent walkthrough' }).click();
assert.equal(await page.getByLabel('Example agent name', { exact: true }).inputValue(), '');
const video = page.video();
await context.close();
await video.saveAs('../qa/agent-creation-preview.webm');
await Promise.all([320, 768, 1440].map(async (width) => {
 const p = await browser.newPage({ viewport: { width, height: 1000 }, reducedMotion: 'reduce' });
 p.on('pageerror', (e) => errors.push(e.message));
 for (const route of ['/', '/agents', '/agents/wren', '/backing', '/providers', '/activity', '/protocol', '/account']) {
  await p.goto(origin + route, { waitUntil: 'networkidle' });
  const geometry = await p.evaluate(() => ({ width: document.body.scrollWidth, viewport: innerWidth, hidden: [...document.querySelectorAll('.reveal-item')].filter((el) => getComputedStyle(el).opacity !== '1').length,
   grids: [...document.querySelectorAll('.dashboard-grid,.creation-preview,.backing-grid,.provider-grid,.account-welcome')].map((el) => [...el.children].slice(0, 2).map((child) => child.getBoundingClientRect().width)) }));
  assert.equal(geometry.width, geometry.viewport, `overflow ${width} ${route}`);
  assert.equal(geometry.hidden, 0, `hidden content ${width} ${route}`);
  if (width === 1440) for (const pair of geometry.grids) assert(Math.abs(pair[0] - pair[1]) < 1, `unequal columns ${route}`);
  if (route === '/') {
   assert.equal(await p.locator('.creation-save.saved').count(), 1, 'reduced motion must show a complete static walkthrough');
   if (width === 320) { await p.locator('.creation-preview').scrollIntoViewIfNeeded(); await p.screenshot({ path: '../qa/agent-creation-mobile.png' }); }
   if (width === 1440) await p.screenshot({ path: '../qa/harmonized-desktop.png', fullPage: true });
  }
 }
 await p.close();
}));
assert.deepEqual(errors, []);
await browser.close();
console.log('Creation playback/pause/replay, no API writes, static reduced motion, equal columns, and eight routes at 320/768/1440px passed.');
