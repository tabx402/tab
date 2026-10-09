import { chromium, expect } from '@playwright/test';
import assert from 'node:assert/strict';

const origin = process.env.TAB_TEST_ORIGIN || 'http://127.0.0.1:5198';
const browser = await chromium.launch();
const page = await browser.newPage({ reducedMotion: 'reduce' });
const errors = [];
page.on('pageerror', error => errors.push(error.message));
let activityCalls = 0;
let firstReleased = false;
let activityFails = false;
let registryFails = true;
let firstActivity;
const firstGate = new Promise(resolve => { firstActivity = resolve; });
const event = (id, kind, status, extra = {}) => ({
  id, agent_id: 'test-agent', agent: 'wren', kind, status,
  message: `fixture event ${id}`, timestamp: '2026-10-06T10:00:00Z', ...extra,
});
let events = [
  event(1, 'run_completed', 'completed'),
  event(2, 'run_completed', 'partial'),
  event(3, 'run_partial', 'partial'),
  event(4, 'model_result', 'unavailable'),
  event(5, 'provider_payment', 'confirmed', { amount: '0.000000000000000001', currency: 'USDT', tx_hash: 'test-tx' }),
  event(6, 'payment_resolved', 'settled', { amount: '0.000000000000000001', currency: 'USDT', tx_hash: 'test-tx' }),
  event(7, 'provider_payment', 'confirmed', { amount: '10', currency: 'USD' }),
];
await page.route('**/api/**', async route => {
  const path = new URL(route.request().url()).pathname;
  let data = [];
  if (path === '/api/config') data = { app_id: null, chain_id: 56, network: 'mainnet', contracts_status: 'not_deployed', financial_actions_enabled: false };
  else if (path === '/api/overview') data = { mode: 'public', summary: { credit_limit: 0, spent: 0, repaid: 0, outstanding: 0, agents: 0 }, agents: [], receipts: [], series: [] };
  else if (path === '/api/metrics') data = { currency: 'USDT', source: 'verified BNB Smart Chain receipts', bonded_agents_earning_payments: 0, completed_commitments: 0, missed_commitments: 0, customer_payments_usdt: '0', buyback_usdt_spent: '0', buyback_tokens_acquired: '0', settlement_status: 'not_deployed' };
  else if (path === '/api/registry') {
    if (registryFails) return route.fulfill({ status: 503, json: { detail: 'fixture registry unavailable' } });
    data = { status: 'not_deployed', chain_id: 56, address: null, agents: [], transactions: [] };
  } else if (path === '/api/activity') {
    activityCalls += 1;
    if (!firstReleased) await firstGate;
    if (activityFails) return route.fulfill({ status: 503, json: { detail: 'fixture activity unavailable' } });
    data = events;
  }
  return route.fulfill({ json: data });
});
try {
  await page.goto(`${origin}/activity`, { waitUntil: 'domcontentloaded' });
  await expect(page.getByText('Loading public activity…', { exact: true })).toBeVisible();
  await expect(page.locator('.log-summary > div').nth(2).locator('strong')).toHaveText('—');
  // Keep the first request pending through a poll. It must not overlap itself.
  await page.waitForTimeout(350);
  const initialCalls = activityCalls;
  await page.waitForTimeout(5200);
  assert.equal(activityCalls, initialCalls);
  firstReleased = true;
  firstActivity();
  await expect(page.locator('.readable-log-row')).toHaveCount(7);
  await expect(page.getByText('updates every 5s', { exact: true })).toBeVisible();
  await expect(page.locator('.log-summary > div').nth(0).locator('strong')).toHaveText('—');
  await expect(page.locator('.log-summary > div').nth(2).locator('strong')).toHaveText('1');
  await expect(page.locator('.log-summary > div').nth(3).locator('strong')).toHaveText('0.000000000000000001 USDT');
  await expect(page.locator('.activity-health > div').nth(1).locator('strong')).toHaveText('2');
  await page.getByLabel('Filter live activity').selectOption('attention');
  await expect(page.locator('.readable-log-row')).toHaveCount(3);
  await page.getByLabel('Filter live activity').selectOption('all');
  // A new event arrives through the normal five-second poll despite registry failure.
  const callsBeforePoll = activityCalls;
  events = [...events, event(8, 'run_started', 'running')];
  await expect(page.locator('.readable-log-row')).toHaveCount(8, { timeout: 8000 });
  assert.ok(activityCalls > callsBeforePoll);
  activityFails = true;
  await page.getByRole('button', { name: 'Refresh activity' }).click();
  await expect(page.getByText('Activity could not be refreshed. Showing the last recorded log.', { exact: true })).toBeVisible();
  await expect(page.locator('.readable-log-row')).toHaveCount(8);
  await expect(page.getByText('connection interrupted', { exact: true })).toBeVisible();
  // Returning to the tab refreshes immediately and a real empty response clears old data.
  activityFails = false;
  registryFails = false;
  events = [];
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect(page.getByText('No public agent activity yet.', { exact: false })).toBeVisible();
  await expect(page.locator('.log-summary > div').nth(0).locator('strong')).toHaveText('0');
  await expect(page.locator('.log-summary > div').nth(2).locator('strong')).toHaveText('0');
  activityFails = true;
  await page.reload({ waitUntil: 'networkidle' });
  await expect(page.getByText('Activity is unavailable right now.', { exact: true })).toBeVisible();
  await expect(page.locator('.log-summary > div').nth(2).locator('strong')).toHaveText('—');
  await expect(page.getByText('No public agent activity yet.', { exact: false })).toHaveCount(0);
  assert.deepEqual(errors, []);
  console.log('Activity polling, source independence, loading/stale/error states, partial outcomes and exact USDT totals passed.');
} finally {
  firstActivity();
  await browser.close();
}
