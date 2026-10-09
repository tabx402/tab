import type { AccountAPI } from "./jobs";

export async function recordSubmitted(api:AccountAPI, path:string, hash:string) {
  if (!/^0x[0-9a-fA-F]{64}$/.test(hash)) throw Error("The submitted transaction hash is invalid.");
  let lastError:unknown;
  // Retry receipt indexing only. A submitted wallet transaction is never sent again here.
  for (let attempt=0;attempt<4;attempt++) {
    try { await api(path,{method:"POST",body:JSON.stringify({tx_hash:hash})});return; }
    catch(error) {lastError=error;if(attempt<3)await new Promise(resolve=>setTimeout(resolve,1000*(attempt+1)));}
  }
  throw Error(`Transaction submitted. Keep its hash and verify confirmation before sending again. ${(lastError as Error)?.message||""}`);
}
