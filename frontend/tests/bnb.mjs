import { chromium } from '../node_modules/playwright/index.mjs';
import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
const origin = process.env.TAB_TEST_ORIGIN || 'http://127.0.0.1:5198';
mkdirSync('../qa', { recursive: true });
const browser = await chromium.launch();
const errors = [];
const models = ['openai/gpt-4.1-mini','openai/gpt-4.1','anthropic/claude-sonnet-4','google/gemini-2.5-flash','deepseek/deepseek-chat-v3-0324'].map(id=>({id,name:id.split('/')[1],provider:id.split('/')[0],available:true,status:'connected',billing:'development credits',max_output_tokens:256}));
const signature='0x'+'4'.repeat(64);
const cfg={app_id:null,chain_id:56,network:'mainnet',contracts_status:'not_deployed',financial_actions_enabled:false,gas_sponsorship_enabled:false,payments_enabled:false,agent_execution_enabled:false};
const page=await browser.newPage({viewport:{width:1440,height:1000},reducedMotion:'reduce'});
page.on('pageerror',e=>errors.push(e.message));
await page.route('**/api/**',route=>{
 const path=new URL(route.request().url()).pathname;
 let data=[];
 if(path==='/api/config')data=cfg;
 else if(path==='/api/overview')data={mode:'public',summary:{credit_limit:0,spent:0,repaid:0,outstanding:0,agents:0},agents:[],receipts:[],series:[]};
 else if(path==='/api/registry')data={status:'not_deployed',chain_id:56,address:null,agents:[],transactions:[]};
 else if(path==='/api/tools')data={'bnb-rpc':true,openrouter:true,tavily:false,x402:true};
 else if(path==='/api/models')data=models;
 else if(path==='/api/jobs/system')data={status:'not_deployed',chain_id:56,network:'mainnet'};
 else if(path==='/api/metrics')data={currency:'USDT',source:'verified BNB Smart Chain receipts',bonded_agents_earning_payments:0,completed_commitments:0,missed_commitments:0,paid_collaborations:0,customer_payments_usdt:'0',fees_collected_usdt:'0',buyback_usdt_spent:'0',buyback_tokens_acquired:'0',settlement_status:'not_deployed'};
 else if(path==='/api/activity')data=[{id:1,agent_id:'CaseSensitiveAgent',agent:'wren',kind:'provider_payment',status:'confirmed',message:'QA fixture payment',timestamp:new Date().toISOString(),amount:'0.01',currency:'USDT',provider:'QA',tx_hash:signature},{id:2,agent_id:'OtherAgent',agent:'willow',kind:'run_completed',status:'completed',message:'QA fixture research result',timestamp:new Date().toISOString()}];
 return route.fulfill({json:data});
});
await page.goto(`${origin}/activity`,{waitUntil:'networkidle'});
assert.equal(await page.getByRole('link',{name:'View payment transaction'}).getAttribute('href'),`https://bscscan.com/tx/${signature}`,'transaction links must target BscScan mainnet');
for(const width of [1440,768,390,320]){
 await page.setViewportSize({width,height:1000});
 for(const path of ['/','/live','/agents','/backing','/providers','/activity','/docs','/protocol','/account']){
  await page.goto(`${origin}${path}`,{waitUntil:'networkidle'});
  assert.equal(/robinhood|usdg|solana|usdc|phantom|\bSOL\b/i.test(await page.locator('body').innerText()),false,`retired chain text ${path}`);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,`overflow ${width} ${path}`);
  assert.equal(await page.locator('.reveal-item').evaluateAll(nodes=>nodes.every(node=>Number(getComputedStyle(node).opacity)>0)),true,`hidden reduced-motion content ${path}`);
  if(path==='/'){
   assert.equal(await page.locator('.landing-hero').count(),1,'home restores the original landing hero');
   assert.match(await page.locator('.landing-hero h1').innerText(),/x402 agent/);
   assert.equal(await page.locator('.home-work-metrics').count(),0,'economic dashboard belongs on the activity page');
   assert.equal(await page.locator('.starter-wallet').count(),1,'the free wallet check is the first task');
   assert.equal(await page.locator('.landing-walkthrough').count(),1,'setup walkthrough stays available in a disclosure');
   await page.locator('.landing-walkthrough summary').click();
   assert.equal(await page.locator('video').count(),1,'the focused landing shows the setup walkthrough');
   assert.equal(await page.locator('video').evaluateAll(nodes=>nodes.every(video=>video.paused)),true,'reduced motion keeps walkthroughs paused');
   assert.equal(await page.locator('.film-frame').evaluateAll(nodes=>nodes.every(node=>node.getBoundingClientRect().width<=800)),true,'walkthroughs remain capped at 800px');
   if([320,768,1440].includes(width)){
    await page.evaluate(()=>window.scrollTo(0,0));
    await page.screenshot({path:`../qa/restored-landing-${width}.png`});
   }
  }
  if(path==='/activity'){
   assert.equal(await page.locator('.home-work-metrics > div').count(),4,'keep verified work metrics on the activity page');
   const metricsBottom=await page.locator('.activity-health').evaluate(node=>node.getBoundingClientRect().bottom);
   assert.equal(await page.locator('.activity-refresh').evaluate((node,bottom)=>node.getBoundingClientRect().top>=bottom,metricsBottom),true,'refresh indicator belongs below the metrics');
   assert.equal(await page.locator('.live-activity .panel-heading .live-indicator').count(),0,'refresh indicator is out of the heading');
   await page.getByLabel('Filter live activity').selectOption('payments');
   assert.equal(await page.locator('.readable-log-row').count(),1,'payment filter excludes research run');
   await page.getByLabel('Search live activity').fill('willow');
   assert.equal(await page.locator('.readable-log-row').count(),0,'search combines with selected payment filter');
   await page.getByText('No activity matches these filters.',{exact:true}).waitFor();
   await page.getByLabel('Filter live activity').selectOption('all');
   assert.equal(await page.locator('.readable-log-row').count(),1,'search finds matching research run');
   await page.getByLabel('Search live activity').fill('');
   if([320,768,1440].includes(width))await page.screenshot({path:`../qa/restored-activity-${width}.png`});
  }
  if(path==='/agents')assert.equal(await page.locator('.garden-stage canvas').count(),1,'full Garden canvas remains available');
 }
 if(width===390)await page.screenshot({path:'../qa/bnb-migration-docs-mobile.png',fullPage:true});
}
await page.route('**/api/metrics',route=>route.fulfill({status:503,json:{detail:'metrics unavailable'}}));
await page.goto(`${origin}/activity`,{waitUntil:'networkidle'});
await page.getByText('Metrics unavailable. Recorded values may be out of date.',{exact:true}).waitFor();
assert.equal(await page.locator('.home-work-metrics dd').first().innerText(),'—','missing metrics must not be presented as verified zeros');
await page.unroute('**/api/metrics');
await page.goto(`${origin}/tests/wizard.html`,{waitUntil:'networkidle'});
await page.getByPlaceholder('e.g. wren').fill('wren');
await page.getByRole('button',{name:'continue',exact:true}).click();
await page.locator('label.tool-choice').filter({hasText:'OpenRouter'}).getByRole('checkbox').check();
await page.locator('label.tool-choice').filter({hasText:'x402'}).getByRole('checkbox').check();
assert.equal(await page.getByLabel('Language model').locator('option').count(),5);
await page.getByLabel('Language model').selectOption('anthropic/claude-sonnet-4');
await page.getByRole('button',{name:'25 USDT',exact:true}).click();
assert.equal(await page.getByLabel('daily limit · USDT',{exact:true}).inputValue(),'25');
await page.getByLabel('maximum per paid call · USDT',{exact:true}).fill('5');
await page.getByRole('button',{name:'1 USDT',exact:true}).click();
assert.equal(await page.getByLabel('maximum per paid call · USDT',{exact:true}).inputValue(),'1','lower daily cap clamps the per-call limit');
await page.getByRole('button',{name:'custom',exact:true}).click();
await page.getByLabel('daily limit · USDT',{exact:true}).fill('25.1234567891234567891');
await page.getByRole('button',{name:'continue',exact:true}).click();
await page.getByRole('alert').waitFor();
assert.equal(await page.getByLabel('daily limit · USDT',{exact:true}).inputValue(),'25.1234567891234567891','invalid precision remains editable rather than rounded');
await page.getByLabel('daily limit · USDT',{exact:true}).fill('25.123456789123456789');
await page.getByLabel('maximum per paid call · USDT',{exact:true}).fill('0.000000000000000001');
await page.getByRole('button',{name:'continue',exact:true}).click();
await page.getByRole('button',{name:'create agent',exact:true}).click();
const created=JSON.parse(await page.evaluate(()=>document.body.dataset.created));
assert.equal(created.model,'anthropic/claude-sonnet-4');
assert.equal(created.tools.includes('x402'),true,'connected x402 provider can be enabled in setup');
assert.equal(created.daily_cap,'25.123456789123456789','custom daily cap preserves all 18 decimals');
assert.equal(created.max_call,'0.000000000000000001','per-call cap preserves one USDT base unit');
await page.getByRole('alert').waitFor();
assert.deepEqual(errors,[]);
await page.screenshot({path:'../qa/bnb-model-selection.png',fullPage:true});
await browser.close();console.log('Restored landing, activity metrics and refresh placement, actual search/filter behavior, unavailable metrics, Garden preservation, cap precision/bounds, BNB Smart Chain receipt links, 9 routes at 4 widths, reduced motion and LLM selection passed.');
