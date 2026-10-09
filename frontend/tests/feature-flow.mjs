import { chromium, expect } from '@playwright/test';
import assert from 'node:assert/strict';
const origin = process.env.TAB_TEST_ORIGIN || 'http://127.0.0.1:5222';
const browser = await chromium.launch();
const page = await browser.newPage({ reducedMotion: 'reduce', viewport: { width: 1440, height: 1000 } });
const errors = [];
page.on('pageerror', error => errors.push(error.message));
let snapshots = 0;
let wrongNetwork = true;
let fail = false;
let verified = false;
let block = '123456';
const pending = { status: 'not_deployed', address: null, reason: 'This module has not been deployed.' };
const active = { status: 'verified', address: '0x2222222222222222222222222222222222222222', reason: 'Verified BNB mainnet contract.' };
await page.route('**/api/**', async route => {
  const url = new URL(route.request().url());
  if (url.pathname === '/api/starter/wallet') {
    snapshots++;
    if (fail) return route.fulfill({ status: 503, json: { detail: 'Public RPC is temporarily unavailable.' } });
    return route.fulfill({ json: { network: 'BNB Smart Chain', chain_id: wrongNetwork ? 1 : 56, source: 'public_rpc', wallet: url.searchParams.get('address'), bnb: '1.234567890123456789', usdt: '12345678901234567890.000000000000000001', block, observed_at: '2026-10-08T12:00:00Z', settled_usdt: false } });
  }
  if (url.pathname === '/api/capabilities') return route.fulfill({ json: { operator_provider_credits: { currency: 'USD', daily_limit: '0.1', used: '0.015', remaining: '0.085', blocked: false, day: '2026-10-09' } } });
  if (url.pathname === '/api/finance/system') return route.fulfill({ json: { chain_id: 56, currency: 'USDT', source: verified ? 'verified_onchain' : 'deployment_pending', modules: { lending: { ...(verified ? active : pending), available_usdt: '10', total_assets_usdt: '20', outstanding_usdt: '10', total_shares: '20' }, stock_loans: { ...pending, assets: [] }, buyback: { ...pending, official_token: null, spent_usdt: '0', tokens_burned: '0' } } } });
  return route.fulfill({ json: [] });
});
try {
  await page.goto(`${origin}/tests/feature-flow.html`, { waitUntil: 'networkidle' });
  await page.getByLabel('wallet address', { exact: true }).fill('invalid');
  await expect(page.getByTestId('agent-wallet-preview')).toHaveCount(0);
  assert.equal(snapshots, 0, 'Invalid address must not reach RPC.');
  await page.getByLabel('wallet address', { exact: true }).fill('0x1111111111111111111111111111111111111111');
  await page.getByRole('button', { name: 'read wallet balances', exact: true }).click();
  await expect(page.getByText('The wallet snapshot could not be verified. Try again.', { exact: true })).toBeVisible();
  await expect(page.locator('[data-testid=agent-wallet-preview] [aria-live=polite]')).toHaveCount(0);
  wrongNetwork = false;
  await page.getByRole('button', { name: 'read wallet balances', exact: true }).click();
  await expect(page.getByTestId('agent-wallet-preview')).toContainText('12,345,678,901,234,567,890.000000000000000001 USDT');
  await expect(page.getByTestId('agent-wallet-preview')).toContainText('block 123,456');
  for (block of ['-1', '1.5', '18446744073709551616', 123456]) {
    await page.getByRole('button', { name: 'read wallet balances', exact: true }).click();
    await expect(page.getByText('The wallet snapshot could not be verified. Try again.', { exact: true })).toBeVisible();
    await expect(page.locator('[data-testid=agent-wallet-preview] [aria-live=polite]')).toHaveCount(0);
  }
  block = '123456';
  fail = true;
  await page.getByRole('button', { name: 'read wallet balances', exact: true }).click();
  await expect(page.getByText('Public RPC is temporarily unavailable.', { exact: true })).toBeVisible();
  await expect(page.locator('[data-testid=agent-wallet-preview] [aria-live=polite]')).toHaveCount(0);
  await expect(page.locator('.run-receipt-top .flow-badge')).toHaveText('some work returned');
  await expect(page.locator('.run-receipt').getByText('No USDT settlement recorded in this run.', { exact: true })).toBeVisible();
  await expect(page.locator('.run-receipt').getByText('0.002 USD in model-provider credits · separate from your USDT budget', { exact: true })).toBeVisible();
  await expect(page.locator('.run-receipt').getByText('0.005 USD total provider-credit cost', { exact: true })).toBeVisible();
  await expect(page.getByText('0.1 USD per day · 0.085 USD remaining today', { exact: true })).toBeVisible();
  await expect(page.locator('.run-receipt').getByText('Review the paid request below. Your wallet authorization is still required.', { exact: true })).toBeVisible();
  const liveProvider = page.locator('.provider-card[data-provider-ready="true"]');
  const unavailableProvider = page.locator('.provider-card[data-provider-ready="false"]');
  await expect(liveProvider.getByRole('link', { name: 'use in an agent' })).toBeVisible();
  await expect(unavailableProvider.getByRole('link', { name: 'use in an agent' })).toHaveCount(0);
  await expect(unavailableProvider.getByRole('link', { name: 'read connection details' })).toBeVisible();
  await expect(page.getByText('awaiting verification', { exact: true })).toHaveCount(3);
  await expect(page.getByRole('button', { name: 'awaiting verified deployment', exact: true })).toBeDisabled();
  verified = true;
  await page.getByRole('button', { name: 'refresh status', exact: true }).click();
  await expect(page.getByRole('link', { name: 'review in my account', exact: true })).toBeVisible();
  await expect(page.getByText('20 USDT', { exact: true })).toBeVisible();
  await page.getByRole('tab', { name: 'stock loans', exact: true }).click();
  await expect(page.getByRole('button', { name: 'awaiting verified deployment', exact: true })).toBeDisabled();
  for (const width of [320, 390, 768]) {
    await page.setViewportSize({ width, height: 844 });
    await page.getByRole('tab', { name: 'lend USDT', exact: true }).click();
    fail = false;
    await page.getByRole('button', { name: 'read wallet balances', exact: true }).click();
    await expect(page.locator('[data-testid=agent-wallet-preview] [aria-live=polite]')).toBeVisible();
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `${width}px layout overflowed`);
  }
  assert.deepEqual(errors, []);
  await page.evaluate(()=>localStorage.setItem('qa:paid-data-reuse','true'));
  await page.reload({waitUntil:'networkidle'});
  await expect(page.getByText('0 USDT spent in this run · previously purchased data reused',{exact:true})).toBeVisible();
  await expect(page.getByText('Original purchase: 2 USDT.',{exact:true})).toBeVisible();
  await expect(page.getByText('2 USDT settled',{exact:true})).toHaveCount(0);
  await expect(page.locator('.receipt-paid-data')).toContainText('Saved paid liquidity data.');
  await expect(page.getByRole('link',{name:'view original payment',exact:true})).toHaveAttribute('href','https://bscscan.com/tx/'+'0x'+'a'.repeat(64));
  await expect(page.getByText('Stored data reused. This run did not authorize another payment.',{exact:true})).toBeVisible();
  assert.deepEqual(errors, []);
  console.log('Integrated live wallet read address/network validation, exact balances, RPC failure states, honest partial/cost/approval receipts, provider availability, finance deployment gates and 320/390/768px layouts passed.');
} finally { await browser.close(); }
