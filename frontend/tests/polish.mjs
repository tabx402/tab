import { chromium, expect } from '@playwright/test';
import assert from 'node:assert/strict';
const origin = process.env.TAB_TEST_ORIGIN || 'http://127.0.0.1:5298';
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, reducedMotion: 'reduce' });
const errors = []; page.on('pageerror', error => errors.push(error.message));
const agent = 'a'.repeat(32), second = 'b'.repeat(32);
const event = (id, kind, status, extra = {}) => ({ id, agent_id: agent, agent: 'wren', kind, status, timestamp: '2026-10-09T10:00:00Z', message: `event ${id}`, ...extra });
const events = [
  event(1, 'run_started', 'running', { run_id: 'run-one' }),
  event(2, 'model_result', 'completed', { run_id: 'run-one', amount: '0.003', currency: 'USD' }),
  event(3, 'run_partial', 'partial', { run_id: 'run-one', preview: { summary: 'A partial research result.' } }),
  event(4, 'run_completed', 'completed', { run_id: 'run-two' }),
  event(5, 'run_completed', 'completed'),
  event(6, 'approval_required', 'requires_authorization'),
  event(7, 'payment', 'confirmed', { amount: '0.000000000000000001', currency: 'USDT', tx_hash: '0x'+'1'.repeat(64) }),
  event(8, 'payment_resolved', 'settled', { amount: '0.000000000000000001', currency: 'USDT', tx_hash: '0x'+'1'.repeat(64) }),
  event(9, 'run_completed', 'completed', { run_id: 'run-one', agent_id: second, agent: 'willow' }),
];
const publicAgent = { id: agent, name: 'wren', registry_id: '1', registry_address: '0x'+'2'.repeat(40), wallet: '0x'+'3'.repeat(40), tools: ['bnb-rpc'], status: 'ready', cadence: 'manual', last_run: '2026-10-09T10:00:00Z', daily_cap: '1', max_call: '0.1' };
let writes = 0;
await page.route('**/api/**', route => {
  if (route.request().method() !== 'GET') { writes++; return route.abort(); }
  const path = new URL(route.request().url()).pathname;
  let data = [];
  if (path === '/api/config') data = { app_id: null, chain_id: 56, network: 'mainnet', contracts_status: 'not_deployed', financial_actions_enabled: false };
  else if (path === '/api/providers') data = [{ id:'bnb-rpc',name:'BNB Smart Chain',category:'data',description:'Read balances.',website:'https://docs.bnbchain.org/',status:'live' },{ id:'tavily',name:'Tavily',category:'data',description:'Read research.',website:'https://tavily.com/',status:'not_connected' },{ id:'x402',name:'x402',category:'data',description:'Paid data.',website:'https://x402.org/',status:'live' }];
  else if (path === '/api/capabilities') data = { operator_provider_credits: { currency: 'USD',daily_limit:'0.1',used:'0.01',remaining:'0.09',blocked:false,day:'2026-10-09' } };
  else if (path === '/api/registry') data = { status:'not_deployed',chain_id:56,address:null,agents:[],transactions:[] };
  else if (path === '/api/activity') data = events;
  else if (path === '/api/agents/live') data = [publicAgent];
  else if (path === '/api/agents/records') data = [{ id:agent,runs:2,failures:0,payments:1,paid:'0.000000000000000001',last_event_id:8 }];
  else if (path === `/api/agents/live/${agent}`) data = { agent: publicAgent, events:[],runs:[{id:'run-one',status:'partial',started_at:'2026-10-09T10:00:00Z'}] };
  else if (path === `/api/agents/live/${agent}/runs/run-one`) data = { status:'partial', output:{summary:'The selected agent returned a partial report.'} };
  else if (path === '/api/metrics') data = {currency:'USDT',source:'verified receipts',completed_commitments:0,missed_commitments:0,customer_payments_usdt:'0',fees_collected_usdt:'0',buyback_usdt_spent:'0',buyback_tokens_acquired:'0',settlement_status:'not_deployed'};
  return route.fulfill({json:data});
});
try {
  await page.goto(origin, {waitUntil:'networkidle'});
  await expect(page.getByText('illustrative example',{exact:true})).toBeVisible();
  await page.getByRole('tab',{name:'01 task'}).focus(); await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab',{name:'02 tools & limits'})).toHaveAttribute('aria-selected','true');
  await expect(page.getByRole('tabpanel')).toContainText('none required');
  await page.keyboard.press('End'); await expect(page.getByRole('tabpanel')).toContainText('This walkthrough does not execute a run.');
  await page.keyboard.press('Control+k'); await expect(page.getByRole('dialog',{name:'Search Tab'})).toBeVisible();
  await page.getByRole('combobox',{name:'Search docs and pages'}).fill('buyback'); await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/\/docs#fees$/); await expect(page.locator('.docs-advanced')).toHaveAttribute('open','');
  assert.equal(await page.locator('#get-started').evaluate(node=>getComputedStyle(node).filter),'none');
  await page.keyboard.press('Control+k'); await page.keyboard.press('Escape'); await expect(page.getByRole('dialog',{name:'Search Tab'})).not.toBeVisible();
  await page.goto(origin+'/providers',{waitUntil:'networkidle'});
  await page.getByRole('button',{name:'research',exact:true}).click(); await expect(page.locator('.provider-card')).toHaveCount(1); await expect(page.locator('.provider-card')).toContainText('connection needed'); await expect(page.locator('.provider-card').getByRole('link',{name:'use in an agent'})).toHaveCount(0);
  await page.getByRole('button',{name:'paid tools',exact:true}).click(); await expect(page.locator('.provider-card')).toContainText('x402');
  await page.goto(origin+'/activity',{waitUntil:'networkidle'});
  await expect(page.locator('.activity-entry')).toHaveCount(7);
  const grouped=page.locator('.activity-entry').filter({hasText:'A partial research result.'});
  await expect(grouped.locator('summary')).toContainText('partial'); await expect(grouped.locator('summary')).toContainText('0.003 USD credits');
  await grouped.locator('summary').click(); await expect(grouped.locator('.activity-detail-event')).toHaveCount(3);
  await expect(page.locator('.log-summary > div').nth(3).locator('strong')).toHaveText('0.000000000000000001 USDT');
  await page.getByLabel('Filter live activity').selectOption('attention'); await expect(page.locator('.activity-entry')).toHaveCount(2);
  await page.goto(origin+'/agents',{waitUntil:'networkidle'});
  await page.getByLabel('Select an agent in the garden').selectOption(agent);
  await expect(page.getByLabel('Selected agent: wren')).toContainText('The selected agent returned a partial report.');
  await expect(page.getByLabel('Selected agent: wren')).toContainText('0.000000000000000001 USDT');
  await expect(page.getByLabel('Selected agent: wren').getByRole('link',{name:'open agent',exact:true})).toHaveAttribute('href',`/agents/${agent}`);
  await page.setViewportSize({width:390,height:844});
  assert.equal(await page.locator('.garden-inspector').evaluate(node=>getComputedStyle(node).position),'fixed');
  await page.getByRole('button',{name:'Close agent details'}).click(); await expect(page.locator('.garden-inspector')).toHaveCount(0);
  await page.getByText('read the garden',{exact:true}).click(); await expect(page.locator('.garden-legend')).toBeVisible();
  await page.getByRole('button',{name:'Open navigation'}).click(); await page.getByRole('navigation',{name:'Mobile navigation'}).getByRole('link',{name:'jobs',exact:true}).click(); await expect(page.locator('#mobile-navigation')).toHaveCount(0);
  for (const width of [320,390,768,1440]) {
    await page.setViewportSize({width,height:900});
    for (const path of ['/','/agents','/providers','/activity','/docs']) {
      await page.goto(origin+path,{waitUntil:'networkidle'});
      const overflowing = await page.evaluate(()=>[...document.querySelectorAll('body *')].filter(node=>node.getBoundingClientRect().right>innerWidth+1 && node.getBoundingClientRect().width>0).map(node=>({tag:node.tagName,class:node.className,right:node.getBoundingClientRect().right})).slice(0,15));
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,`overflow ${width} ${path}: ${JSON.stringify(overflowing)}`);
      assert.equal(await page.locator('header .brand path').first().evaluate(node=>Number.parseFloat(getComputedStyle(node).strokeDashoffset)),0,'reduced motion sprout remains visible');
    }
  }
  assert.equal(writes,0); assert.deepEqual(errors,[]);
  console.log('Navigation, keyboard search, advanced docs, walkthrough, purpose filters, exact run grouping, settlement dedupe, garden selection and responsive reduced motion passed.');
} finally { await browser.close(); }
