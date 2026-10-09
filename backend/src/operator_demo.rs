//! Local operator CLI for one bounded, explicitly labelled team demonstration.
//! This has no HTTP route and always uses a separate database from production.
use crate::{error::{ApiError,Result},models::*,AppState};
use serde_json::{json,Value};
use std::io::{BufRead, Write};
const OWNER:&str="operator:tab-team-demo";
const WALLET:&str="0x00e128e7779ea927a087b40ade268dedc1b34a90";

pub async fn serve(state:AppState)->Result<()> {
    if std::env::var("TAB_DEMO_AUTHORIZATION").as_deref()!=Ok("bnb56-team-demo-max-1usdt-1usd-0.005bnb")
        || !state.config.database.file_name().is_some_and(|v|v=="tab-team-demo.sqlite") {
        return Err(ApiError::forbidden("Use the isolated team demonstration database and exact budget authorization."));
    }
    for line in std::io::stdin().lock().lines() {
        let line=line.map_err(|_|ApiError::internal())?;
        if line.len()>32768 { return Err(ApiError::validation("Demo request is too large.")); }
        let input:Value=serde_json::from_str(&line)?;
        let result=dispatch(&state,input).await;
        let output=match result {Ok(value)=>json!({"ok":true,"data":value}),Err(e)=>json!({"ok":false,"status":e.0.as_u16(),"message":e.1})};
        println!("{}",output);
        std::io::stdout().flush().map_err(|_|ApiError::internal())?;
    }
    Ok(())
}
async fn dispatch(state:&AppState,input:Value)->Result<Value> {
    let id=input["id"].as_str().unwrap_or("");
    match input["op"].as_str().unwrap_or("") {
        "create"=>{
            let name=input["name"].as_str().unwrap_or("tab demo wren");
            if !["tab demo wren","tab demo finch"].contains(&name) { return Err(ApiError::validation("Choose an explicitly labelled team demonstration agent.")); }
            let plan:AgentInput=serde_json::from_value(json!({"name":name,"purpose":"Team-funded Tab demonstration. Report the observed BNB chain and dev-wallet balances. Do not imply customer activity or investment performance.","tools":["bnb-rpc","openrouter","x402"],"daily_cap":"1","max_call":"0.1","cadence":"manual","model":"openai/gpt-4.1-mini","public_activity":true}))?;
            Ok(serde_json::to_value(state.create_agent_once(OWNER,plan,Some(if name=="tab demo wren"{"tab-team-demo-wren-v1"}else{"tab-team-demo-finch-v1"}))?)?)
        }
        "challenge"=>state.challenge(OWNER,id,WALLET).await,
        "register"=>{
            let proof:WalletProof=serde_json::from_value(input["proof"].clone())?;
            if proof.wallet.to_lowercase()!=WALLET { return Err(ApiError::forbidden("Wrong team demonstration wallet.")); }
            Ok(serde_json::to_value(state.register(OWNER,id,proof).await?)?)
        }
        "run"=>Ok(serde_json::to_value(state.run_agent(OWNER,id).await?)?),
        "quote"=>state.x402_quote(OWNER,id,"dexscreener-usdt-market-data").await,
        "execute"=>state.x402_execute(OWNER,id,input["quote_id"].as_str().unwrap_or(""),input["signature"].as_str().unwrap_or("")).await,
        "reconcile"=>state.x402_reconcile(OWNER,id,input["quote_id"].as_str().unwrap_or(""),input["tx_hash"].as_str().unwrap_or("")).await,
        "history"=>state.x402_history(OWNER,id),
        "agents"=>Ok(serde_json::to_value(state.store.list_agents(OWNER)?)?),
        _=>Err(ApiError::validation("Unsupported demonstration operation."))
    }
}
