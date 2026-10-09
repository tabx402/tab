import {useState} from 'react';
import {createRoot} from 'react-dom/client';
import {BrowserRouter} from 'react-router-dom';
import '@fontsource/dm-sans/400.css';
import '@fontsource/dm-sans/500.css';
import '../src/styles.css';
import {AgentJobs} from '../src/components/Jobs';
import type {RuntimeAgent,Job,JobIntent} from '../src/lib/api';
import type {AccountAPI} from '../src/lib/jobs';
const fixture=await fetch('/fixture').then(r=>r.json()) as {agents:RuntimeAgent[],jobs:Job[],walletMode?:boolean};
let pending:JobIntent|null=null;
function Preview(){
 const [agent,setAgent]=useState(fixture.agents[0]);
 const api:AccountAPI=async<T,>(path:string,options?:RequestInit):Promise<T>=>{
  if(path.startsWith('/account/jobs?'))return fixture.jobs as T;
  if(path==='/account/job-actions')return (pending?[{intent:pending,confirmed:0,tx_hash:null}]:[]) as T;
  if(fixture.walletMode&&path.endsWith('/prepare')){const body=JSON.parse(String(options?.body));pending={id:'qa-job-intent',job_id:path.split('/')[3],action:body.action,chain_id:56,sender:fixture.agents[0].wallet!,to:'0x2222222222222222222222222222222222222222',data:'0x12345678',value:'0x0',transactions:[],expires_at:new Date(Date.now()+600000).toISOString()} as JobIntent;return pending as T;}
  if(fixture.walletMode&&path.endsWith('/submitted')){document.body.dataset.submitted=String(options?.body);return {status:'submitted'} as T;}
  if(fixture.walletMode&&path.endsWith('/confirm')){if(!document.body.dataset.submitted)throw Error('submission must be recorded first');pending=null;return fixture.jobs[0] as T;}
  if(options?.method==='POST') {document.body.dataset.input=String(options.body);throw Error('QA example. No transaction or job saved.');}
  throw Error('unexpected fixture request');
 };
 // Stable API identity keeps the component's refresh effect aligned with real account behavior.
 return <BrowserRouter><main><section className="panel"><p>local QA example · no funds</p><button onClick={()=>setAgent(fixture.agents[1])}>switch agent</button><AgentJobs agent={agent} owned={fixture.agents} api={api} sendTransaction={async()=>{const mode=localStorage.getItem('qa:job-send-mode');if(mode==='rejected')throw Object.assign(Error('Wallet request rejected.'),{code:4001});if(mode==='unknown')throw Error('RPC response lost.');throw Error('QA cannot sign');}}/></section></main></BrowserRouter>;
}
createRoot(document.getElementById('root')!).render(<Preview/>);
