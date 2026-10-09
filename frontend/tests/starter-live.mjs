import { chromium, expect } from '@playwright/test';
import assert from 'node:assert/strict';
const origin=process.env.TAB_TEST_ORIGIN||'https://tabagents.io';
const browser=await chromium.launch();
const page=await browser.newPage({viewport:{width:390,height:844},reducedMotion:'reduce'});
const errors=[];page.on('pageerror',e=>errors.push(e.message));
try {
  await page.goto(origin,{waitUntil:'networkidle'});
  await expect(page.getByRole('link',{name:'create an agent',exact:true})).toBeVisible();
  await expect(page.getByRole('heading',{name:'check a BNB wallet.'})).toHaveCount(0);
  await expect(page.locator('#starter-address')).toHaveCount(0);
  const response=await page.request.get(origin+'/api/starter/wallet?address=0x00e128e7779ea927a087b40ade268dedc1b34a90');
  assert.equal(response.status(),200);const data=await response.json();
  assert.equal(data.chain_id,56);assert.equal(data.source,'public_rpc');assert.equal(data.settled_usdt,false);assert.match(data.block,/^\d+$/);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  assert.deepEqual(errors,[]);
  console.log('Live homepage leads to agent creation, standalone lookup is absent and integrated wallet-data API remains available.');
} finally {await browser.close();}
