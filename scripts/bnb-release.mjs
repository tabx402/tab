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
if(!['plan','deploy','verify'].includes(mode))throw Error('Usage: node scripts/bnb-release.mjs plan|deploy|verify');
const directory=path.join(root,'contracts/deployments');fs.mkdirSync(directory,{recursive:true});
const manifestPath=path.join(directory,'bnb-56.json');
const planPath=path.join(directory,'bnb-56-plan.json');
const journalPath=path.join(directory,'bnb-56-transactions.json');
const USDT='0x55d398326f99059fF775485246999027B3197955';
const DEPLOYER='0x00E128E7779EA927a087B40AdE268Dedc1B34A90';
const USDT_HASH='0x97a48aa4c129657440dafdacd4c836389734d28cc4a0ca7403e68da660a74a59';
const rpc=process.env.TAB_BNB_RPC||'https://bsc-dataseed.bnbchain.org';
const client=createPublicClient({chain:bsc,transport:http(rpc,{timeout:20000,retryCount:1})});
const modules=[['protocol','TabProtocol',6000000n],['backing','TabBacking',2300000n],['economics','TabEconomics',4500000n]];
const artifacts=Object.fromEntries(modules.map(([key,name])=>[key,JSON.parse(fs.readFileSync(path.join(root,`contracts/bnb/out/${name}.sol/${name}.json`),'utf8'))]));
const sourceHash=keccak256('0x'+Buffer.from(JSON.stringify(modules.map(([k])=>[k,artifacts[k].bytecode.object,artifacts[k].deployedBytecode.object]))).toString('hex'));
function write(file,data){const temp=file+'.tmp';fs.writeFileSync(temp,JSON.stringify(data,null,2)+'\n',{mode:0o644});fs.renameSync(temp,file);}
function read(file){return JSON.parse(fs.readFileSync(file,'utf8'));}
function eq(a,b){return typeof a==='string'&&typeof b==='string'&&a.toLowerCase()===b.toLowerCase();}
async function base(){if(await client.getChainId()!==56)throw Error('RPC chain must be56');const code=await client.getCode({address:USDT});if(!code||keccak256(code)!==USDT_HASH||await client.readContract({address:USDT,abi:erc20Abi,functionName:'decimals'})!==18)throw Error('USDT contract mismatch');}
function sourceMatches(code,a){if(!code)return false;let observed=code.slice(2);let expected=a.deployedBytecode.object.replace(/^0x/,'');if(observed.length!==expected.length)return false;for(const ranges of Object.values(a.deployedBytecode.immutableReferences||{})){for(const{start,length}of ranges){const offset=start*2,size=length*2;observed=observed.slice(0,offset)+'0'.repeat(size)+observed.slice(offset+size);expected=expected.slice(0,offset)+'0'.repeat(size)+expected.slice(offset+size);}}return observed===expected;}
async function verify(m){await base();if(m.chain_id!==56||m.source_hash!==sourceHash||!eq(m.authority,DEPLOYER))throw Error('Manifest/source/authority mismatch');for(const[key]of modules){const address=m.contracts[key].address;const code=await client.getCode({address});if(!sourceMatches(code,artifacts[key])||keccak256(code)!==m.contracts[key].code_hash)throw Error(`${key} bytecode mismatch`);}
const address=m.contracts.protocol.address;const p=await client.readContract({address,abi:artifacts.protocol.abi,functionName:'getProtocol'});if(!eq(p.usdt,USDT)||!eq(p.authority,DEPLOYER)||!eq(p.backing,m.contracts.backing.address)||!eq(p.economics,m.contracts.economics.address)||!eq(p.tabToken,'0x'+'0'.repeat(40))||p.feeBps!==200)throw Error('Protocol wiring mismatch');
for(const key of ['backing','economics']){const parent=await client.readContract({address:m.contracts[key].address,abi:artifacts[key].abi,functionName:'protocol'});if(!eq(parent,address))throw Error('Module parent mismatch');const token=await client.readContract({address:m.contracts[key].address,abi:artifacts[key].abi,functionName:'usdt'});if(!eq(token,USDT))throw Error('Module USDT mismatch');}return {chain_id:56,contracts:m.contracts,verified_at:new Date().toISOString()};}
await base();
if(mode==='verify'){console.log(JSON.stringify(await verify(read(manifestPath)),null,2));process.exit(0);}
if(mode==='plan'){
 if(fs.existsSync(journalPath))throw Error('Existing transaction journal: resume original deployment plan or verify; do not regenerate addresses');
 if(fs.existsSync(manifestPath)&&read(manifestPath).status==='deployed')throw Error('Deployment already exists; verify it rather than create another');
 const nonce=await client.getTransactionCount({address:DEPLOYER,blockTag:'pending'});const latest=await client.getTransactionCount({address:DEPLOYER,blockTag:'latest'});if(nonce!==latest)throw Error('Deployer has pending transactions');
 const gasPrice=await client.getGasPrice();if(gasPrice>1000000000n)throw Error('Gas price exceeds1gwei');
 const addresses=Object.fromEntries(modules.map(([k],i)=>[k,getContractAddress({from:DEPLOYER,nonce:BigInt(nonce+i)})]));
 const transactions=modules.map(([key,,gas],i)=>({key,nonce:nonce+i,gas:gas.toString(),to:null,value:'0',data:encodeDeployData({abi:artifacts[key].abi,bytecode:artifacts[key].bytecode.object,args:key==='protocol'?[USDT,DEPLOYER]:[addresses.protocol]}),address:addresses[key]}));
 transactions.push({key:'configure',nonce:nonce+3,gas:'160000',to:addresses.protocol,value:'0',data:encodeFunctionData({abi:artifacts.protocol.abi,functionName:'configureModules',args:[addresses.backing,addresses.economics]})});
 const maxCost=transactions.reduce((sum,t)=>sum+BigInt(t.gas)*gasPrice,0n);if(maxCost>parseEther('0.005'))throw Error('Deployment exceeds0.005BNB hard cap');
 const plan={chain_id:56,deployer:DEPLOYER,source_hash:sourceHash,created_at:new Date().toISOString(),gas_price_wei:gasPrice.toString(),gas_budget_bnb:'0.005',maximum_cost_bnb:formatEther(maxCost),transactions};write(planPath,plan);console.log(JSON.stringify({plan:planPath,addresses,maximum_cost_bnb:plan.maximum_cost_bnb,hard_cap_bnb:plan.gas_budget_bnb,source_hash:sourceHash},null,2));process.exit(0);
}
const plan=read(planPath);
if(plan.source_hash!==sourceHash||plan.chain_id!==56||!eq(plan.deployer,DEPLOYER))throw Error('Plan no longer matches compiled artifacts');
if(Date.now()-Date.parse(plan.created_at)>3600000&&!fs.existsSync(journalPath))throw Error('Plan expired; review fresh gas and nonce');
const baseNonce=plan.transactions?.[0]?.nonce;if(!Number.isSafeInteger(baseNonce)||baseNonce<0||plan.transactions.length!==4)throw Error('Invalid transaction sequence');
const plannedAddresses=Object.fromEntries(modules.map(([key],i)=>[key,getContractAddress({from:DEPLOYER,nonce:BigInt(baseNonce+i)})]));
for(let i=0;i<4;i++){const t=plan.transactions[i];const key=i<3?modules[i][0]:'configure';const to=i<3?null:plannedAddresses.protocol;const data=i<3?encodeDeployData({abi:artifacts[key].abi,bytecode:artifacts[key].bytecode.object,args:i===0?[USDT,DEPLOYER]:[plannedAddresses.protocol]}):encodeFunctionData({abi:artifacts.protocol.abi,functionName:'configureModules',args:[plannedAddresses.backing,plannedAddresses.economics]});const gas=i<3?modules[i][2].toString():'160000';if(t.key!==key||t.to!==to||t.data!==data||t.value!=='0'||t.nonce!==baseNonce+i||t.gas!==gas||(i<3&&t.address!==plannedAddresses[key]))throw Error('Plan transaction differs from source-derived deployment flow');}
if(!process.env.TAB_DEPLOYER_KEY)throw Error('Inject project deployer alias through Ryan Vault');
const key=process.env.TAB_DEPLOYER_KEY;const account=privateKeyToAccount(key.startsWith('0x')?key:`0x${key}`);delete process.env.TAB_DEPLOYER_KEY;
if(!eq(account.address,DEPLOYER))throw Error('Wrong project deployer');
const gasPrice=BigInt(plan.gas_price_wei);const total=plan.transactions.reduce((sum,t)=>sum+BigInt(t.gas)*gasPrice,0n);
if(total>parseEther('0.005')||gasPrice>1000000000n||await client.getBalance({address:DEPLOYER})<total)throw Error('Gas budget or balance check failed');
const wallet=createWalletClient({account,chain:bsc,transport:http(rpc,{retryCount:0,timeout:20000})});
const journal=fs.existsSync(journalPath)?read(journalPath):{source_hash:sourceHash,transactions:[]};if(journal.source_hash!==sourceHash)throw Error('Previous deployment journal belongs to different source');
for(const t of plan.transactions){let sent=journal.transactions.find(r=>r.key===t.key);if(!sent){const nonce=await client.getTransactionCount({address:DEPLOYER,blockTag:'pending'});if(nonce!==t.nonce)throw Error('Deployer nonce changed; reconcile before signing');
const request={account,to:t.to||undefined,data:t.data,value:0n,nonce:t.nonce,gas:BigInt(t.gas),gasPrice,type:'legacy'};
const estimate=await client.estimateGas(request);if(estimate>BigInt(t.gas))throw Error(`${t.key} estimate exceeds planned gaslimit`);
const hash=await wallet.sendTransaction(request);sent={key:t.key,hash,address:t.address||null};journal.transactions.push(sent);write(journalPath,journal);console.log(JSON.stringify({broadcast:t.key,hash}));}
const receipt=await client.waitForTransactionReceipt({hash:sent.hash,confirmations:3,timeout:120000});const observed=await client.getTransaction({hash:sent.hash});if(!eq(observed.from,DEPLOYER)||observed.nonce!==t.nonce||observed.input!==t.data||observed.value!==0n||(t.to?!eq(observed.to,t.to):observed.to!==null)||observed.gas!==BigInt(t.gas)||observed.gasPrice!==gasPrice||observed.chainId!==56)throw Error('Journal receipt does not match planned transaction');if(receipt.status!=='success')throw Error(`${t.key} reverted; stop and reconcile`);sent.block=receipt.blockNumber.toString();sent.gas_used=receipt.gasUsed.toString();sent.gas_cost_wei=(receipt.gasUsed*receipt.effectiveGasPrice).toString();write(journalPath,journal);
if(t.address){if(!eq(receipt.contractAddress,t.address))throw Error('Contract address mismatch');const code=await client.getCode({address:t.address});if(!sourceMatches(code,artifacts[t.key]))throw Error('Deployed bytecode differs from compiled source');}}
const contracts={};for(const[key]of modules){const tx=journal.transactions.find(t=>t.key===key);const code=await client.getCode({address:tx.address});contracts[key]={address:tx.address.toLowerCase(),code_hash:keccak256(code),transaction_hash:tx.hash,block_number:Number(tx.block)};}
const m={status:'deployed',chain_id:56,network:'BNB Smart Chain',usdt_address:USDT.toLowerCase(),usdt_decimals:18,usdt_code_hash:USDT_HASH,authority:DEPLOYER.toLowerCase(),official_tab_address:null,source_hash:sourceHash,deployment_block:contracts.protocol.block_number,contracts,deployed_at:new Date().toISOString(),gas_cost_bnb:formatEther(journal.transactions.reduce((sum,t)=>sum+BigInt(t.gas_cost_wei),0n))};
await verify(m);write(manifestPath,m);console.log(JSON.stringify({manifest:manifestPath,contracts,gas_cost_bnb:m.gas_cost_bnb},null,2));
