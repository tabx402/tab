import assert from 'node:assert/strict';
import {chromium} from '@playwright/test';
const origin=process.env.TAB_TEST_ORIGIN||'http://127.0.0.1:5236';
const browser=await chromium.launch();
const page=await browser.newPage();
const input={name:'wren',purpose:'check published chain data',daily_cap:'1',max_call:'0.1',tools:['bnb-rpc']};
await page.route(origin+'/',route=>route.fulfill({contentType:'text/html',body:'<!doctype html><title>Account retry test</title>'}));
const key=owner=>page.evaluate(async({owner,input})=>(await import('/src/lib/account.ts')).creationRequest(owner,input),{owner,input});
try {
  await page.goto(origin);
  const first=await key('account-a');
  assert.equal(await key('account-a'),first,'repeated submissions must recover one draft');
  await page.reload();
  assert.equal(await key('account-a'),first,'lost-response recovery must survive reload');
  assert.notEqual(await key('account-b'),first,'request identity must be account scoped');
  const changed=await page.evaluate(async input=>(await import('/src/lib/account.ts')).creationRequest('account-a',{...input,name:'finch'}),input);
  assert.notEqual(changed,first,'a different plan needs a different request key');
  await page.evaluate(async()=>(await import('/src/lib/account.ts')).finishCreation('account-a'));
  assert.notEqual(await key('account-a'),first,'completed creation must allow a later identical agent');
  await page.evaluate(()=>{Storage.prototype.setItem=function(){throw Error('storage disabled')};});
  const memory=await key('private-browser');
  assert.equal(await key('private-browser'),memory,'storage failures must preserve in-memory retry protection');
  console.log('Account creation: duplicate clicks, lost-response reload, account isolation, changed plans, completed requests and unavailable browser storage passed.');
} finally {await browser.close();}
