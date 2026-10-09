#!/usr/bin/env python3
"""Preserve BNB accounts and history after the reviewed identity-only v2 import."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import sqlite3
import time
import urllib.request

LEGACY = "0x567e7187d477b1a68c3ac3d292ad44a0d5e770c7"
USDT = "0x55d398326f99059ff775485246999027b3197955"
RPC = "https://bsc-dataseed.bnbchain.org"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def rpc(method, params):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    request = urllib.request.Request(RPC, data=body, headers={"Content-Type": "application/json"})
    result = json.load(urllib.request.urlopen(request, timeout=20))
    require("error" not in result, "BNB verification failed")
    return result["result"]


def call(address, data, block="latest"):
    return rpc("eth_call", [{"to": address, "data": data}, block])


def verify_chain(old, new):
    require(rpc("eth_chainId", []) == "0x38", "Migration requires chain 56")
    require(old["contracts"]["protocol"]["address"].lower() == LEGACY, "Unexpected legacy registry")
    require(new.get("previous_protocol", "").lower() == LEGACY, "New registry has no verified legacy link")
    require(new.get("fee_bps") == 50 and new.get("official_tab_address") is None, "Wrong fee or official token")
    target = new["contracts"]["protocol"]["address"].lower()
    require(target != LEGACY, "Migration needs a distinct verified deployment")
    for manifest in (old, new):
        require(manifest["chain_id"] == 56 and manifest["usdt_address"].lower() == USDT, "Wrong network or currency")
        for module in ("protocol", "backing", "economics"):
            contract = manifest["contracts"][module]
            code = rpc("eth_getCode", [contract["address"], "latest"])
            require(code != "0x" and rpc("web3_sha3", [code]) == contract["code_hash"], "Contract code changed")
    require(call(target, "0x3524c61e")[-40:].lower() == LEGACY[2:], "Wrong immutable migration source")
    ids = new["migrated_agents"]
    require(0 < len(ids) <= 100 and len(ids) == len(set(ids)), "Invalid identity batch")
    tx_hash = new["migration_transaction"]
    tx = rpc("eth_getTransactionByHash", [tx_hash])
    receipt = rpc("eth_getTransactionReceipt", [tx_hash])
    data = "0x408caabf" + f"{32:064x}" + f"{len(ids):064x}" + "".join(i[2:] for i in ids)
    require(tx["to"].lower() == target and tx["from"].lower() == new["authority"].lower()
            and int(tx["value"], 16) == 0 and tx["input"].lower() == data.lower(), "Migration transaction differs from reviewed import")
    require(receipt["status"] == "0x1", "Migration reverted")
    block = rpc("eth_getBlockByNumber", [receipt["blockNumber"], False])
    head = int(rpc("eth_blockNumber", []), 16)
    require(block["hash"] == receipt["blockHash"] and head - int(receipt["blockNumber"], 16) >= 2, "Migration is not canonically confirmed")
    # Compare both registries at one recent confirmed block. BNB's public RPC
    # prunes historical state; activation needs the identities to match now,
    # including any owner policy change since the import transaction.
    state_block = hex(head - 2)
    state_header = rpc("eth_getBlockByNumber", [state_block, False])
    require(state_header and state_header.get("hash"), "Confirmed state block is unavailable")
    for agent_id in ids:
        payload = "0xa6c2af01" + agent_id[2:]
        before = call(LEGACY, payload, state_block)
        after = call(target, payload, state_block)
        require(before == after, "Migration changed an owner, policy, pause flag or spending counter")
        raw = bytes.fromhex(after[2:])
        offset = int.from_bytes(raw[:32], "big")
        owner = "0x" + raw[offset + 12:offset + 32].hex()
        require(owner == agent_id[:42].lower(), "Identity owner mismatch")
    require(rpc("eth_getBlockByNumber", [state_block, False])["hash"] == state_header["hash"], "Confirmed state block changed during verification")
    return target, set(i.lower() for i in ids), tx_hash


def planned_rows(db, target, ids):
    for query in (
        "SELECT COUNT(*) FROM jobs",
        "SELECT COUNT(*) FROM wallet_intents WHERE confirmed=0 AND (tx_hash IS NOT NULL OR julianday(json_extract(payload,'$.expires_at'))>julianday('now'))",
        "SELECT COUNT(*) FROM sponsor_jobs WHERE status='pending'",
        "SELECT COUNT(*) FROM agent_runs WHERE status='running'",
        "SELECT COUNT(*) FROM x402_quotes WHERE status NOT IN ('confirmed','failed','expired')",
    ):
        require(db.execute(query).fetchone()[0] == 0, "An existing commitment or pending action needs a separate migration")
    rows = []
    for agent_id, raw in db.execute("SELECT id,payload FROM runtime_agents"):
        agent = json.loads(raw)
        if agent["registry_address"].lower() == target:
            continue
        require(agent["registry_address"].lower() == LEGACY, "Unrelated registry in this database")
        if agent.get("registry_id"):
            require(agent["registry_id"].lower() in ids, "An existing agent was not imported")
            require(agent.get("wallet", "").lower() == agent["registry_id"][:42].lower(), "Stored wallet differs from identity")
        else:
            require(agent["status"] == "awaiting_registration", "Unexpected unregistered agent state")
        agent["registry_address"] = target
        rows.append((agent_id, raw, json.dumps(agent)))
    return rows


def apply_rows(db, rows, transaction):
    at = datetime.now(timezone.utc).isoformat()
    for agent_id, old, new in rows:
        changed = db.execute("UPDATE runtime_agents SET payload=? WHERE id=? AND payload=?", (new, agent_id, old)).rowcount
        require(changed == 1, "Agent changed concurrently; migration rolled back")
        # Only imported onchain identities have a confirmed migration receipt.
        # Drafts merely change their future registration target.
        if json.loads(new).get("registry_id"):
            db.execute("INSERT INTO agent_events(agent_id,kind,status,at,message,tx_hash) VALUES(?,?,?,?,?,?)",
                       (agent_id, "registration_migrated", "confirmed", at, "Agent identity carried to the 0.5% secured-credit deployment.", transaction))
        else:
            db.execute("DELETE FROM wallet_challenges WHERE agent_id=?", (agent_id,))
            db.execute("DELETE FROM sponsor_challenges WHERE agent_id=?", (agent_id,))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("database", type=Path)
    parser.add_argument("previous_manifest", type=Path)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    require(args.database.is_file() and not args.database.is_symlink(), "Use the existing BNB database")
    target, ids, transaction = verify_chain(json.loads(args.previous_manifest.read_text()), json.loads(args.manifest.read_text()))
    db = sqlite3.connect(args.database.as_uri() + ("?mode=rw" if args.apply else "?mode=ro"), uri=True, timeout=15)
    rows = planned_rows(db, target, ids)
    if not args.apply:
        print(json.dumps({"status": "reviewed", "agents": [r[0] for r in rows], "transaction": transaction, "target": target}))
        return
    require(os.environ.get("TAB_V2_DATABASE_AUTHORIZATION") == "preserve-bnb56-agent-history", "Review the dry run and stop the API before applying the migration")
    os.umask(0o077)
    backup = args.database.with_name(args.database.name + ".before-v2-" + str(int(time.time())))
    with sqlite3.connect(backup) as copy:
        db.backup(copy)
    try:
        db.execute("BEGIN IMMEDIATE")
        require(planned_rows(db, target, ids) == rows, "Database changed since verification")
        apply_rows(db, rows, transaction)
        db.commit()
    except BaseException:
        db.rollback()
        raise
    print(json.dumps({"status": "migrated", "agents": [r[0] for r in rows], "backup": str(backup), "transaction": transaction}))


if __name__ == "__main__":
    main()
