import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
MODULE=Path(__file__).resolve().parents[1]/'migrate-agent-settings.py'
spec=importlib.util.spec_from_file_location('migration',MODULE)
migration=importlib.util.module_from_spec(spec)
spec.loader.exec_module(migration)
class SettingsMigrationTests(unittest.TestCase):
 def test_settings_preserved_without_registration_keys_receipts_or_fixture_accounts(self):
  with tempfile.TemporaryDirectory() as folder:
   source,dest=(Path(folder)/name for name in ['old.sqlite','bnb.sqlite'])
   for p in [source,dest]:
    with sqlite3.connect(p) as d:
     for t in ['runtime_agents','agent_plans']:d.execute(f'CREATE TABLE {t}(id TEXT PRIMARY KEY,owner TEXT,payload TEXT,created_at TEXT)')
   with sqlite3.connect(dest) as d:d.execute('CREATE TABLE network_identity(chain TEXT)');d.execute("INSERT INTO network_identity VALUES('eip155:56:usdt18')")
   payload={'name':'wren','purpose':'read published data','tools':['robinhood-rpc','priors-x402'],'daily_cap':5,'max_call':'0.100001','wallet':'old-wallet','registration_tx':'old-hash','token_address':'old-token','status':'ready','next_run':'scheduled','private_key':'must-not-copy'}
   with sqlite3.connect(source) as d:
    d.execute('INSERT INTO runtime_agents VALUES(?,?,?,?)',('a'*32,'did:privy:user',json.dumps(payload),'2026-01-01'))
    d.execute('INSERT INTO runtime_agents VALUES(?,?,?,?)',('b'*32,'test-fixture',json.dumps(payload),'2026-01-01'))
   original=source.read_bytes();r=migration.migrate(source,dest,'0x'+'1'*40);self.assertEqual(r['agents'],1);self.assertEqual(source.read_bytes(),original)
   with sqlite3.connect(dest) as d:p=json.loads(d.execute('SELECT payload FROM runtime_agents').fetchone()[0])
   self.assertEqual(p['tools'],['bnb-rpc','x402']);self.assertEqual(p['max_call'],'0.100001');self.assertEqual(p['status'],'awaiting_registration')
   for key in ['wallet','registration_tx','token_address','next_run']:self.assertIsNone(p[key])
   self.assertNotIn('private_key',p);self.assertEqual(migration.migrate(source,dest,'0x'+'1'*40)['agents'],0)
 def test_wrong_network_never_imports(self):
  with tempfile.TemporaryDirectory() as folder:
   source,dest=(Path(folder)/name for name in ['old.sqlite','bnb.sqlite']);source.touch()
   with sqlite3.connect(dest) as d:d.execute('CREATE TABLE network_identity(chain TEXT)');d.execute("INSERT INTO network_identity VALUES('other')")
   with self.assertRaises(ValueError):migration.migrate(source,dest,'0x'+'1'*40)
