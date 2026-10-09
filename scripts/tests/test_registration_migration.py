import importlib.util
import json
from pathlib import Path
import sqlite3
import unittest

spec = importlib.util.spec_from_file_location("migration", Path(__file__).parents[1] / "migrate-bnb-registration.py")
migration = importlib.util.module_from_spec(spec)
spec.loader.exec_module(migration)
TARGET = "0x" + "2" * 40
WALLET = "0x" + "1" * 40
IDENTITY = WALLET + "3" * 24


class RegistrationMigrationTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.executescript("""
          CREATE TABLE runtime_agents(id TEXT PRIMARY KEY,owner TEXT,payload TEXT);
          CREATE TABLE jobs(id TEXT);
          CREATE TABLE wallet_intents(confirmed INTEGER,tx_hash TEXT,payload TEXT);
          CREATE TABLE sponsor_jobs(status TEXT);
          CREATE TABLE agent_runs(status TEXT);
          CREATE TABLE x402_quotes(status TEXT);
          CREATE TABLE agent_events(agent_id TEXT,kind TEXT,status TEXT,at TEXT,message TEXT,tx_hash TEXT);
          CREATE TABLE wallet_challenges(agent_id TEXT);
          CREATE TABLE sponsor_challenges(agent_id TEXT);
          CREATE TABLE preserved_history(kind TEXT,payload TEXT);
        """)
        self.agent = {"id": "wren", "registry_address": migration.LEGACY, "registry_id": IDENTITY,
                      "wallet": WALLET, "status": "paused", "registration_tx": "old-proof", "daily_cap": "1"}
        self.db.execute("INSERT INTO runtime_agents VALUES('wren','private-owner',?)", (json.dumps(self.agent),))
        self.db.execute("INSERT INTO preserved_history VALUES('payment','canonical paid receipt')")
        self.db.commit()

    def tearDown(self):
        self.db.close()

    def test_identity_move_preserves_original_receipt_owner_and_state(self):
        rows = migration.planned_rows(self.db, TARGET, {IDENTITY})
        migration.apply_rows(self.db, rows, "migration-proof")
        owner, raw = self.db.execute("SELECT owner,payload FROM runtime_agents").fetchone()
        self.assertEqual(owner, "private-owner")
        self.assertEqual(json.loads(raw), {**self.agent, "registry_address": TARGET})
        self.assertEqual(self.db.execute("SELECT payload FROM preserved_history").fetchone()[0], "canonical paid receipt")
        self.assertEqual(migration.planned_rows(self.db, TARGET, {IDENTITY}), [])

    def test_unknown_agent_blocks_the_whole_migration(self):
        with self.assertRaisesRegex(ValueError, "not imported"):
            migration.planned_rows(self.db, TARGET, set())
        self.assertEqual(json.loads(self.db.execute("SELECT payload FROM runtime_agents").fetchone()[0]), self.agent)

    def test_pending_wallet_authorization_blocks_migration(self):
        self.db.execute("INSERT INTO wallet_intents VALUES(0,'submitted-hash','{}')")
        with self.assertRaisesRegex(ValueError, "pending action"):
            migration.planned_rows(self.db, TARGET, {IDENTITY})

    def test_drafts_keep_settings_but_require_fresh_registration_permission(self):
        draft = {**self.agent, "id": "draft", "registry_id": None, "registration_tx": None,
                 "wallet": None, "status": "awaiting_registration"}
        self.db.execute("INSERT INTO runtime_agents VALUES('draft','another-owner',?)", (json.dumps(draft),))
        self.db.execute("INSERT INTO wallet_challenges VALUES('draft')")
        self.db.execute("INSERT INTO sponsor_challenges VALUES('draft')")
        migration.apply_rows(self.db, migration.planned_rows(self.db, TARGET, {IDENTITY}), "migration-proof")
        moved = json.loads(self.db.execute("SELECT payload FROM runtime_agents WHERE id='draft'").fetchone()[0])
        self.assertEqual(moved, {**draft, "registry_address": TARGET})
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM wallet_challenges").fetchone()[0], 0)
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM sponsor_challenges").fetchone()[0], 0)
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM agent_events WHERE agent_id='draft'").fetchone()[0], 0)
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM agent_events WHERE kind='registration_migrated'").fetchone()[0], 1)

    def test_concurrent_plan_change_does_not_get_overwritten(self):
        rows = migration.planned_rows(self.db, TARGET, {IDENTITY})
        changed = {**self.agent, "daily_cap": "2"}
        self.db.execute("UPDATE runtime_agents SET payload=?", (json.dumps(changed),))
        with self.assertRaisesRegex(ValueError, "concurrently"):
            migration.apply_rows(self.db, rows, "migration-proof")
        self.assertEqual(json.loads(self.db.execute("SELECT payload FROM runtime_agents").fetchone()[0]), changed)


if __name__ == "__main__":
    unittest.main()
