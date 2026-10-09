import { chromium } from '../node_modules/playwright/index.mjs';
import { mkdirSync } from 'node:fs';
const origin=process.env.TAB_TEST_ORIGIN || 'http://127.0.0.1:5198';
mkdirSync('../qa/bnb-recordings',{recursive:true});
const browser=await chromium.launch();
for(const film of ['setup','activity','tools'].filter(film=>!process.env.TAB_RECORD_FILM||film===process.env.TAB_RECORD_FILM)){
 const context=await browser.newContext({viewport:{width:1440,height:1000},recordVideo:{dir:'../qa/bnb-recordings',size:{width:1440,height:1000}},reducedMotion:'reduce'});
 const page=await context.newPage();
 await page.route('**/api/**',route=>{
  const path=new URL(route.request().url()).pathname;
  const providers=[{id:'bnb-rpc',name:'BNB Smart Chain RPC',category:'data',description:'Read BNB Smart Chain wallet balances and finalized chain observations. BNB pays fees; USDT funds supported paid work.',website:'https://www.bnbchain.org',status:'live'},{id:'openrouter',name:'OpenRouter',category:'inference',description:'Choose an approved language model for your agent. A connected provider account is required.',website:'https://openrouter.ai',status:'not_connected'},{id:'tavily',name:'Tavily',category:'data',description:'Web research and cited search results. A connected provider account is required.',website:'https://tavily.com',status:'not_connected'}];
  const data=path==='/api/config'?{app_id:null,chain_id:56,network:'mainnet',financial_actions_enabled:false,contracts_status:'live',gas_sponsorship_enabled:false,payments_enabled:true,agent_execution_enabled:true}:path==='/api/overview'?{mode:'public',summary:{credit_limit:0,spent:0,repaid:0,outstanding:0,agents:0},agents:[],receipts:[],series:[]}:path==='/api/tools'?{'bnb-rpc':true,openrouter:false,tavily:false}:path==='/api/registry'?{status:'live',agents:[],transactions:[]}:path==='/api/providers'?providers:path==='/api/metrics'?{currency:'USDT',source:'verified BNB Smart Chain receipts',bonded_agents_earning_payments:0,completed_commitments:0,missed_commitments:0,customer_payments_usdt:'0',buyback_usdt_spent:'0',buyback_tokens_acquired:'0',settlement_status:'live'}:[];
  return route.fulfill({json:data});
 });
 await page.goto(`${origin}${film==='setup'?'/tests/wizard.html?sponsored=true':film==='tools'?'/providers':'/activity'}`,{waitUntil:'networkidle'});
 await page.evaluate(()=>{const x=document.createElement('div');x.textContent='interface preview · no transactions sent';Object.assign(x.style,{position:'fixed',bottom:'18px',right:'24px',zIndex:'10000',font:'11px monospace',color:'#94a8b6',background:'#091521',border:'1px solid #293b49',borderRadius:'4px',padding:'8px 12px'});document.body.append(x);});
 if(film==='setup'){
  await page.getByPlaceholder('e.g. wren').fill('willow');await page.waitForTimeout(1500);
  await page.getByRole('button',{name:'continue',exact:true}).click();
  await page.locator('select').first().selectOption('hourly');await page.waitForTimeout(2500);
  await page.getByRole('button',{name:'continue',exact:true}).click();await page.waitForTimeout(3000);
 }else if(film==='activity'){
  await page.waitForTimeout(2000);await page.getByLabel('Filter live activity').selectOption('payments');await page.waitForTimeout(2000);await page.getByLabel('Filter live activity').selectOption('all');await page.waitForTimeout(2000);
 }else{await page.waitForTimeout(2000);await page.getByRole('button',{name:'inference',exact:true}).click();await page.waitForTimeout(2000);await page.getByRole('button',{name:'all',exact:true}).click();await page.waitForTimeout(2000);}
 await page.screenshot({path:`../qa/bnb-recordings/${film}-poster.png`});const video=page.video();await context.close();await video.saveAs(`../qa/bnb-recordings/${film}.webm`);console.log(`Recorded BNB Smart Chain ${film}`);
}
await browser.close();
