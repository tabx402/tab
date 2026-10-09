#!/usr/bin/env python3
"""Carry account-owned settings into BNB drafts, never chain authority or funds.

Requires a destination initialized by tab-api --init-database. The source is
opened read-only. Existing BNB rows are left intact; only real Privy owners move.
"""
import argparse
import json
import pathlib
import re
import sqlite3
from decimal import Decimal

TOOLS = {"robinhood-rpc":"bnb-rpc", "solana-rpc":"bnb-rpc", "bnb-rpc":"bnb-rpc", "priors-x402":"x402", "x402":"x402", "openrouter":"openrouter", "tavily":"tavily", "web-search":"web-search"}

def migrate(source: pathlib.Path, destination: pathlib.Path, protocol: str) -> dict:
    if source.resolve() == destination.resolve() or source.is_symlink() or destination.is_symlink():
        raise ValueError("Separate regular source and BNB destination files are required.")
    if not source.is_file() or not destination.is_file() or not re.fullmatch(r"0x[0-9a-fA-F]{40}", protocol):
        raise ValueError("Existing databases and the exact BNB protocol address are required.")
    counts = {"agents":0,"plans":0,"existing_or_non_account":0}
    with sqlite3.connect(source.as_uri()+"?mode=ro",uri=True) as old, sqlite3.connect(destination) as new:
        if new.execute("SELECT chain FROM network_identity").fetchall() != [("eip155:56:usdt18",)]:
            raise ValueError("Destination is not the initialized BNB database.")
        new.execute("BEGIN IMMEDIATE")
        for table in ("runtime_agents", "agent_plans"):
            exists = old.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?",(table,)).fetchone()
            if not exists:
                continue
            for identifier,owner,payload,created in old.execute(f"SELECT id,owner,payload,created_at FROM {table}"):
                if not owner.startswith("did:privy:") or new.execute(f"SELECT 1 FROM {table} WHERE id=?",(identifier,)).fetchone():
                    counts["existing_or_non_account"] += 1
                    continue
                if not re.fullmatch(r"[0-9a-f]{32}",identifier):
                    raise ValueError("A saved setup has an unsupported identifier; no rows imported.")
                original=json.loads(payload,parse_float=Decimal)
                original=original.get("plan",original)
                cap=Decimal(str(original["daily_cap"]))
                if not 1 <= cap <= 10000:
                    raise ValueError("A saved setup needs its daily cap reviewed before import.")
                item={"name":original["name"],"purpose":original["purpose"],"daily_cap":str(cap),"id":identifier,"created_at":created}
                if table=="agent_plans":
                    item.update(providers=original["providers"],status="saved")
                    key="plans"
                else:
                    tools=list(dict.fromkeys(TOOLS[t] for t in original["tools"] if t in TOOLS))
                    if not tools:
                        raise ValueError("A saved agent has no supported BNB tools.")
                    maximum=Decimal(str(original["max_call"]))
                    if not 0 < maximum <= min(cap,Decimal(10)):
                        raise ValueError("A saved per-call cap needs review before import.")
                    item.update(template=original.get("template","onchain"),tools=tools,cadence=original.get("cadence","manual"),max_call=str(maximum),model=original.get("model","openai/gpt-4.1-mini"),watch_address=None,report_agent=original.get("report_agent",437),public_activity=original.get("public_activity",False),token_address=None,wallet=None,registry_address=protocol.lower(),registry_id=None,registration_tx=None,status="awaiting_registration",last_run=None,next_run=None)
                    key="agents"
                new.execute(f"INSERT INTO {table}(id,owner,payload,created_at) VALUES(?,?,?,?)",(identifier,owner,json.dumps(item,separators=(",",":")),created))
                counts[key]+=1
        new.commit()
    return counts

if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("source",type=pathlib.Path)
    parser.add_argument("destination",type=pathlib.Path)
    parser.add_argument("protocol")
    args=parser.parse_args()
    print(json.dumps(migrate(args.source.absolute(),args.destination.absolute(),args.protocol)))
