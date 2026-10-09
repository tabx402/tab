#!/usr/bin/env node
/** Public BNB deployment plan, bounded signing, and source/chain verification.
 * Signing consumes TAB_DEPLOYER_KEY from Ryan Vault only. No customer funds move.
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {createPublicClient,createWalletClient,http,getContractAddress,encodeDeployData,encodeFunctionData,keccak256,formatEther,parseEther,erc20Abi} from '../frontend/node_modules/viem/_esm/index.js';
import {privateKeyToAccount} from '../frontend/node_modules/viem/_esm/accounts/index.js';
import {bsc} from '../frontend/node_modules/viem/_esm/chains/index.js';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const mode=process.argv[2];
if(!['plan','deploy','verify'].includes(mode))throw Error('Usage: node scripts/bnb-release-v2.mjs plan|deploy|verify');
const directory=path.join(root,'contracts/deployments');fs.mkdirSync(directory,{recursive:true});
const manifestPath=path.join(directory,'bnb-56-v2.json');
const planPath=path.join(directory,'bnb-56-v2-plan.json');
const journalPath=path.join(directory,'bnb-56-v2-transactions.json');
const privatePath=path.join(root,'backend/data/bnb-v2-deployment-private.json');
const USDT='0x55d398326f99059fF775485246999027B3197955';
const DEPLOYER='0x00E128E7779EA927a087B40AdE268Dedc1B34A90';
const LEGACY='0x567e7187d477b1a68c3ac3d292ad44a0d5e770c7';
const previous=JSON.parse(fs.readFileSync(path.join(directory,'bnb-56-legacy.json'),'utf8'));
if(!eq(previous.contracts.protocol.address,LEGACY))throw Error('Previous deployment changed; review the migration source');
const USDT_HASH='0x97a48aa4c129657440dafdacd4c836389734d28cc4a0ca7403e68da660a74a59';
const rpc=process.env.TAB_BNB_RPC||'https://bsc-dataseed.bnbchain.org';
const client=createPublicClient({chain:bsc,transport:http(rpc,{timeout:20000,retryCount:1})});
const modules=[['protocol','TabProtocol',6000000n],['backing','TabBacking',4000000n],['economics','TabEconomics',4500000n]];
const artifacts=Object.fromEntries(modules.map(([key,name])=>[key,JSON.parse(fs.readFileSync(path.join(root,`contracts/bnb/out/${name}.sol/${name}.json`),'utf8'))]));
const sourceHash=keccak256('0x'+Buffer.from(JSON.stringify(modules.map(([k])=>[k,artifacts[k].bytecode.object,artifacts[k].deployedBytecode.object]))).toString('hex'));
function write(file,data){const temp=file+'.tmp';fs.writeFileSync(temp,JSON.stringify(data,null,2)+'\n',{mode:0o644});fs.renameSync(temp,file);}
function writePrivate(data){fs.mkdirSync(path.dirname(privatePath),{recursive:true,mode:0o700});const temp=privatePath+'.tmp';fs.writeFileSync(temp,JSON.stringify(data),{mode:0o600});fs.chmodSync(temp,0o600);fs.renameSync(temp,privatePath);}
function read(file){return JSON.parse(fs.readFileSync(file,'utf8'));}
function eq(a,b){return typeof a==='string'&&typeof b==='string'&&a.toLowerCase()===b.toLowerCase();}
async function base(){if(await client.getChainId()!==56)throw Error('RPC chain must be56');const code=await client.getCode({address:USDT});if(!code||keccak256(code)!==USDT_HASH||await client.readContract({address:USDT,abi:erc20Abi,functionName:'decimals'})!==18)throw Error('USDT contract mismatch');}
function sourceMatches(code,a){if(!code)return false;let observed=code.slice(2);let expected=a.deployedBytecode.object.replace(/^0x/,'');if(observed.length!==expected.length)return false;for(const ranges of Object.values(a.deployedBytecode.immutableReferences||{})){for(const{start,length}of ranges){const offset=start*2,size=length*2;observed=observed.slice(0,offset)+'0'.repeat(size)+observed.slice(offset+size);expected=expected.slice(0,offset)+'0'.repeat(size)+expected.slice(offset+size);}}return observed===expected;}
function expectedTransactions(nonce,ids) {
 const addresses=Object.fromEntries(modules.map(([key],i)=>[key,getContractAddress({from:DEPLOYER,nonce:BigInt(nonce+i)})]));
 const tx=modules.map(([key,,gas],i)=>({key,nonce:nonce+i,gas:gas.toString(),to:null,value:'0',data:encodeDeployData({abi:artifacts[key].abi,bytecode:artifacts[key].bytecode.object,args:key==='protocol'?[USDT,DEPLOYER]:[addresses.protocol]}),address:addresses[key]}));
 for(const [key,method,args,gas] of [['legacy','configureLegacy',[LEGACY],160000n],['configure','configureModules',[addresses.backing,addresses.economics],160000n],['import','importAgents',[ids],200000n+BigInt(ids.length)*200000n]]) {
  tx.push({key,nonce:nonce+tx.length,gas:String(gas),to:addresses.protocol,value:'0',data:encodeFunctionData({abi:artifacts.protocol.abi,functionName:method,args})});
 }
 return tx;
}
async function inventory() {
 for(const [key] of modules) {
  const address=previous.contracts[key].address;
  const code=await client.getCode({address});
  if(!code||keccak256(code)!==previous.contracts[key].code_hash)throw Error('Previous '+key+' bytecode changed');
  const balance=await client.readContract({address:USDT,abi:erc20Abi,functionName:'balanceOf',args:[address]});
  if(balance!==0n||await client.getBalance({address})!==0n)throw Error('Existing balances need an explicit financial migration before activation');
 }
 const assets=JSON.parse(fs.readFileSync(path.join(root,'backend/config/backing-assets.json'),'utf8'));
 for(const asset of assets)for(const key of ['backing','economics']) {
  const liability=await client.readContract({address:previous.contracts[key].address,abi:artifacts[key].abi,functionName:'tokenLiability',args:[asset.address]});
  if(liability!==0n)throw Error('Existing token custody must remain reachable in a separately reviewed migration');
 }
 const response=await fetch('https://tabagents.io/api/registry');
 if(!response.ok)throw Error('Cannot read the confirmed legacy registry');
 const registry=await response.json();
 if(registry.status!=='live'||!eq(registry.address,LEGACY)||!registry.agents?.length||registry.agents.length>100)throw Error('Legacy registry must be current and bounded');
 const ids=[...new Set(registry.agents.map(a=>a.id.toLowerCase()))].sort();
 const agents=[];
 for(const id of ids){const state=await client.readContract({address:LEGACY,abi:artifacts.protocol.abi,functionName:'getAgent',args:[id]});if(!eq(state.owner,id.slice(0,42)))throw Error('Registry ownership mismatch');agents.push({id,...state});}
 return {ids,agents:JSON.parse(JSON.stringify(agents,(_,v)=>typeof v==='bigint'?v.toString():v)),observed_at:new Date().toISOString()};
}
async function verify(m){await base();if(m.chain_id!==56||m.source_hash!==sourceHash||!eq(m.authority,DEPLOYER))throw Error('Manifest/source/authority mismatch');for(const[key]of modules){const address=m.contracts[key].address;const code=await client.getCode({address});if(!sourceMatches(code,artifacts[key])||keccak256(code)!==m.contracts[key].code_hash)throw Error(`${key} bytecode mismatch`);}
const address=m.contracts.protocol.address;const legacy=await client.readContract({address,abi:artifacts.protocol.abi,functionName:'legacyProtocol'});if(!eq(legacy,LEGACY))throw Error('Migration registry differs from the reviewed source');const p=await client.readContract({address,abi:artifacts.protocol.abi,functionName:'getProtocol'});if(!eq(p.usdt,USDT)||!eq(p.authority,DEPLOYER)||!eq(p.backing,m.contracts.backing.address)||!eq(p.economics,m.contracts.economics.address)||!eq(p.tabToken,'0x'+'0'.repeat(40))||p.feeBps!==50)throw Error('Protocol wiring mismatch');
for(const key of ['backing','economics']){const parent=await client.readContract({address:m.contracts[key].address,abi:artifacts[key].abi,functionName:'protocol'});if(!eq(parent,address))throw Error('Module parent mismatch');const token=await client.readContract({address:m.contracts[key].address,abi:artifacts[key].abi,functionName:'usdt'});if(!eq(token,USDT))throw Error('Module USDT mismatch');}const collateral=await client.readContract({address:m.contracts.backing.address,abi:artifacts.backing.abi,functionName:'collateralAssets',args:[USDT]});if(collateral[1]!==9000||collateral[2]!==9500||collateral[5])throw Error('USDT collateral policy mismatch');for(const id of m.migrated_agents){const [before,after]=await Promise.all([client.readContract({address:LEGACY,abi:artifacts.protocol.abi,functionName:'getAgent',args:[id]}),client.readContract({address,abi:artifacts.protocol.abi,functionName:'getAgent',args:[id]})]);if(!eq(before.owner,after.owner))throw Error('Migrated ownership mismatch');}return {chain_id:56,contracts:m.contracts,migrated_agents:m.migrated_agents,verified_at:new Date().toISOString()};}
await base();
if(mode==='verify'){console.log(JSON.stringify(await verify(read(manifestPath)),null,2));process.exit(0);}
if(mode==='plan'){
 if(fs.existsSync(journalPath))throw Error('Existing transaction journal: resume original deployment plan or verify; do not regenerate addresses');
 if(fs.existsSync(manifestPath)&&read(manifestPath).status==='deployed')throw Error('Deployment already exists; verify it rather than create another');
 const nonce=await client.getTransactionCount({address:DEPLOYER,blockTag:'pending'});const latest=await client.getTransactionCount({address:DEPLOYER,blockTag:'latest'});if(nonce!==latest)throw Error('Deployer has pending transactions');
 const gasPrice=await client.getGasPrice();if(gasPrice>1000000000n)throw Error('Gas price exceeds1gwei');
 const migration=await inventory();
 const transactions=expectedTransactions(nonce,migration.ids);
 const addresses=Object.fromEntries(transactions.filter(t=>t.address).map(t=>[t.key,t.address]));
 const maxCost=transactions.reduce((sum,t)=>sum+BigInt(t.gas)*gasPrice,0n);if(maxCost>parseEther('0.005'))throw Error('Deployment exceeds0.005BNB hard cap');
 const plan={chain_id:56,migration,deployer:DEPLOYER,source_hash:sourceHash,created_at:new Date().toISOString(),gas_price_wei:gasPrice.toString(),gas_budget_bnb:'0.005',maximum_cost_bnb:formatEther(maxCost),transactions};write(planPath,plan);console.log(JSON.stringify({plan:planPath,addresses,maximum_cost_bnb:plan.maximum_cost_bnb,hard_cap_bnb:plan.gas_budget_bnb,migrated_agents:migration.ids,source_hash:sourceHash},null,2));process.exit(0);
}
const plan=read(planPath);
if(plan.source_hash!==sourceHash||plan.chain_id!==56||!eq(plan.deployer,DEPLOYER))throw Error('Plan no longer matches compiled artifacts');
if(Date.now()-Date.parse(plan.created_at)>3600000&&!fs.existsSync(journalPath))throw Error('Plan expired; review fresh gas and nonce');
const baseNonce=plan.transactions?.[0]?.nonce;
if(!Number.isSafeInteger(baseNonce)||baseNonce<0||!Array.isArray(plan.migration?.ids)||!plan.migration.ids.length||plan.migration.ids.length>100)throw Error('Invalid migration plan');
if(JSON.stringify(plan.transactions)!==JSON.stringify(expectedTransactions(baseNonce,plan.migration.ids)))throw Error('Transactions differ from the exact source-derived migration');
if(!fs.existsSync(journalPath)){const current=await inventory();if(JSON.stringify(current.ids)!==JSON.stringify(plan.migration.ids))throw Error('Registry changed; review a fresh migration plan');}
if(process.env.TAB_V2_DEPLOY_AUTHORIZATION!=='bnb56-secured-credit-v2-max-0.005-bnb')throw Error('Review the v2 plan before authorizing a new immutable deployment. This does not switch the live site.');
if(!process.env.TAB_DEPLOYER_KEY)throw Error('Inject project deployer alias through Ryan Vault');
const key=process.env.TAB_DEPLOYER_KEY;const account=privateKeyToAccount(key.startsWith('0x')?key:`0x${key}`);delete process.env.TAB_DEPLOYER_KEY;
if(!eq(account.address,DEPLOYER))throw Error('Wrong project deployer');
const gasPrice=BigInt(plan.gas_price_wei);const total=plan.transactions.reduce((sum,t)=>sum+BigInt(t.gas)*gasPrice,0n);
if(total>parseEther('0.005')||gasPrice>1000000000n||await client.getBalance({address:DEPLOYER})<total)throw Error('Gas budget or balance check failed');
const wallet=createWalletClient({account,chain:bsc,transport:http(rpc,{retryCount:0,timeout:20000})});
const privateJournal=fs.existsSync(privatePath)?read(privatePath):{source_hash:sourceHash,transactions:[]};if(privateJournal.source_hash!==sourceHash)throw Error('Private journal belongs to different source');
const journal=fs.existsSync(journalPath)?read(journalPath):{source_hash:sourceHash,transactions:[]};if(journal.source_hash!==sourceHash)throw Error('Previous deployment journal belongs to different source');
for(const t of plan.transactions){let sent=journal.transactions.find(r=>r.key===t.key);if(!sent){const nonce=await client.getTransactionCount({address:DEPLOYER,blockTag:'pending'});if(nonce!==t.nonce)throw Error('Deployer nonce changed; reconcile before signing');
const request={account,to:t.to||undefined,data:t.data,value:0n,nonce:t.nonce,gas:BigInt(t.gas),gasPrice,type:'legacy'};
const estimate=await client.estimateGas(request);if(estimate>BigInt(t.gas))throw Error(`${t.key} estimate exceeds planned gaslimit`);
let signed=privateJournal.transactions.find(r=>r.key===t.key);
if(!signed){const raw=await wallet.signTransaction(request);signed={key:t.key,hash:keccak256(raw),raw};privateJournal.transactions.push(signed);writePrivate(privateJournal);}
sent={key:t.key,hash:signed.hash,address:t.address||null};journal.transactions.push(sent);write(journalPath,journal);
try{await client.sendRawTransaction({serializedTransaction:signed.raw});}catch{console.log(JSON.stringify({pending:t.key,hash:signed.hash,message:'Reconcile this exact signed transaction before any retry.'}));}
console.log(JSON.stringify({broadcast:t.key,hash:signed.hash}));}else{const known=await client.getTransaction({hash:sent.hash}).catch(()=>null);if(!known){const signed=privateJournal.transactions.find(r=>r.key===t.key);if(!signed||signed.hash!==sent.hash||keccak256(signed.raw)!==sent.hash)throw Error('Cannot recover the exact signed deployment');await client.sendRawTransaction({serializedTransaction:signed.raw}).catch(()=>{});}}
const receipt=await client.waitForTransactionReceipt({hash:sent.hash,confirmations:3,timeout:120000});const observed=await client.getTransaction({hash:sent.hash});if(!eq(observed.from,DEPLOYER)||observed.nonce!==t.nonce||observed.input!==t.data||observed.value!==0n||(t.to?!eq(observed.to,t.to):observed.to!==null)||observed.gas!==BigInt(t.gas)||observed.gasPrice!==gasPrice||observed.chainId!==56)throw Error('Journal receipt does not match planned transaction');if(receipt.status!=='success')throw Error(`${t.key} reverted; stop and reconcile`);sent.block=receipt.blockNumber.toString();sent.gas_used=receipt.gasUsed.toString();sent.gas_cost_wei=(receipt.gasUsed*receipt.effectiveGasPrice).toString();write(journalPath,journal);
if(t.address){if(!eq(receipt.contractAddress,t.address))throw Error('Contract address mismatch');const code=await client.getCode({address:t.address});if(!sourceMatches(code,artifacts[t.key]))throw Error('Deployed bytecode differs from compiled source');}}
const contracts={};for(const[key]of modules){const tx=journal.transactions.find(t=>t.key===key);const code=await client.getCode({address:tx.address});contracts[key]={address:tx.address.toLowerCase(),code_hash:keccak256(code),transaction_hash:tx.hash,block_number:Number(tx.block)};}
const m={status:'deployed',chain_id:56,network:'BNB Smart Chain',usdt_address:USDT.toLowerCase(),usdt_decimals:18,usdt_code_hash:USDT_HASH,authority:DEPLOYER.toLowerCase(),official_tab_address:null,fee_bps:50,credit_mode:'collateralized',holder_exemption_scope:'executor wallet balance at settlement',previous_protocol:LEGACY,migrated_agents:plan.migration.ids,migration_transaction:journal.transactions.find(t=>t.key==='import').hash,source_hash:sourceHash,deployment_block:contracts.protocol.block_number,contracts,deployed_at:new Date().toISOString(),gas_cost_bnb:formatEther(journal.transactions.reduce((sum,t)=>sum+BigInt(t.gas_cost_wei),0n))};
await verify(m);write(manifestPath,m);console.log(JSON.stringify({manifest:manifestPath,contracts,gas_cost_bnb:m.gas_cost_bnb},null,2));
