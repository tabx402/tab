"""Publisher regressions using temporary files and mocked network/service commands.

Run with: python3 -m unittest discover -s scripts/tests -p 'test_*.py'
"""

import fcntl
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile
import time
import unittest


PROJECT = Path(__file__).resolve().parents[2]

MOCK_COMMAND = r'''#!/usr/bin/python3
import json, os, pathlib, subprocess, sys
from urllib.parse import urlsplit

command = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
fixture = json.loads(os.environ["RELEASE_TEST_FIXTURE"])
base = pathlib.Path(fixture["base"])
root = pathlib.Path(fixture["root"])
web = pathlib.Path(fixture["web"])
unit = pathlib.Path(fixture["unit"])
old_web = pathlib.Path(fixture["old_web"])
scenario = os.environ.get("RELEASE_TEST_FAILURE", "")
with (base / "commands.jsonl").open("a") as log:
    log.write(json.dumps({"command":command,"args":args}) + "\n")

def fail_once(name):
    marker = base / (name + ".fired")
    if marker.exists():
        return False
    marker.touch()
    return True

if command == "curl":
    url = next(arg for arg in reversed(args) if arg.startswith(("https://", "http://")))
    path = urlsplit(url).path
    new_api = "current-api/bin/tab-api" in unit.read_text()
    if path == "/api/health":
        print(json.dumps({"status":"ok","backend":"rust" if new_api else "python"}))
    elif path == "/api/config":
        value = {"backend":"rust","chain_id":56,"usdt_address":"0x55d398326f99059ff775485246999027b3197955","usdt_decimals":18,"contracts_status":"live","financial_actions_enabled":True}
        if new_api:
            manifest = json.loads((root / "current-api/contracts/deployments/bnb-56.json").read_text())
            value["official_tab_address"] = manifest.get("official_tab_address")
            value["holder_access_enabled"] = manifest.get("holder_access_enabled", False)
        if scenario == "holder_access" and new_api:
            value["holder_access_enabled"] = False
        if scenario == "config" and new_api:
            value["chain_id"] = 97
        print(json.dumps(value))
    else:
        current = (web / "current").resolve()
        if path in ("", "/"):
            output = (current / "index.html").read_bytes()
            if scenario == "public_index" and current != old_web:
                output = b"stale public index"
        else:
            target = current / path.lstrip("/")
            if not target.is_file():
                sys.exit(22)
            output = target.read_bytes()
            if scenario == "public_asset" and path.endswith(".js") and current != old_web:
                output += b"wrong cached bundle"
            if scenario == "public_asset_http" and path.endswith(".css") and current != old_web:
                sys.exit(22)
        sys.stdout.buffer.write(output)
    sys.exit(0)

if command == "systemctl":
    if args[:2] == ["restart", "tabagents-api"] and "current-api/bin/tab-api" in unit.read_text():
        if scenario == "startup" and fail_once("startup"):
            sys.exit(1)
    sys.exit(0)

# Every mutation target stays inside this fixture. Strip ownership flags only:
# testing filesystem permissions must not require root or change real ownership.
if command == "chown":
    for arg in args[1:]:
        assert pathlib.Path(arg).is_relative_to(base), arg
    sys.exit(0)

if command in ("install", "ln", "mv"):
    cleaned = []
    index = 0
    while index < len(args):
        arg = args[index]
        if command == "install" and arg in ("-o", "-g"):
            index += 2
            continue
        cleaned.append(arg)
        index += 1
    for arg in cleaned:
        if arg.startswith("/"):
            assert pathlib.Path(arg).is_relative_to(base), arg
    if command == "install" and scenario == "service_install" and args[-1] == str(unit) and "previous-api.service" not in args[-2] and fail_once("service_install"):
        sys.exit(1)
    if command == "ln" and scenario == "frontend_link" and args[-1] == str(web / "current.next") and pathlib.Path(args[-2]) != old_web and fail_once("frontend_link"):
        sys.exit(1)
    if command == "mv" and args[-1] == str(web / "current"):
        source = pathlib.Path(args[-2])
        if scenario == "frontend_switch" and source.resolve() != old_web and fail_once("frontend_switch"):
            sys.exit(1)
    sys.exit(subprocess.run(["/usr/bin/" + command, *cleaned]).returncode)
raise RuntimeError("Unexpected mock command: " + command)
'''


class PublisherTests(unittest.TestCase):
    tag = "candidate-01"

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="tab-publisher-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.root = self.base / "project"
        self.web = self.base / "web"
        self.unit = self.base / "etc/systemd/system/tabagents-api.service"
        self.operator = self.base / "etc/tabagents"
        self.mock_bin = self.base / "mock-bin"
        self.old_web = self.web / "releases/previous"
        self.old_api = self.root / "releases/previous"
        self.legacy_unit = "[Service]\nDescription=legacy API\nExecStart=/usr/bin/true\n"
        for directory in (self.old_web, self.old_api, self.unit.parent, self.operator, self.mock_bin):
            directory.mkdir(parents=True, exist_ok=True)
        self.write(self.old_web / "index.html", "previous frontend")
        self.unit.write_text(self.legacy_unit)
        (self.web / "current").symlink_to(self.old_web, target_is_directory=True)
        (self.root / "current-api").symlink_to(self.old_api, target_is_directory=True)
        (self.operator / "api.env").write_text("# isolated credential-free fixture\n")

        self.paths = {
            "/home/ubuntu/apps/tabagents": str(self.root),
            "/var/www/tabagents": str(self.web),
            "/etc/systemd/system/tabagents-api.service": str(self.unit),
            "/etc/tabagents/api.env": str(self.operator / "api.env"),
            "/etc/tabagents/bnb.env": str(self.operator / "bnb.env"),
            "/etc/tabagents/sponsor.env": str(self.operator / "sponsor.env"),
        }
        source = (PROJECT / "scripts/release-tab.sh").read_text()
        for dangerous in ("/usr/bin/curl", "/bin/curl", "/usr/bin/systemctl", "/bin/systemctl"):
            self.assertNotIn(dangerous, source, "Unmocked network/service command")
        self.script = self.root / "scripts/release-tab.sh"
        self.write(self.script, self.rewrite(source))
        self.script.chmod(0o755)
        self.write(self.root / "scripts/check-release.py", (PROJECT / "scripts/check-release.py").read_text())
        self.write(self.root / "deploy/tabagents-production.service", self.rewrite((PROJECT / "deploy/tabagents-production.service").read_text()))
        self.write(self.root / "backend/target/release/tab-api", "#!/bin/sh\nexit 0\n")
        (self.root / "backend/target/release/tab-api").chmod(0o755)
        self.write(self.root / "frontend/dist/index.html", '<!doctype html><script type="module" src="/assets/app.js"></script><link rel="stylesheet" href="/assets/app.css">')
        for path, content in {"assets/app.js":"console.log('new app');", "assets/app.css":"body{color:white}", "assets/nested/lazy.js":"export default 1;", "images/a-picture.svg":"<svg/>", "media/demo.mp4":"video fixture"}.items():
            self.write(self.root / "frontend/dist" / path, content)
        for name in ("job-merchants", "x402-merchants", "backing-assets"):
            self.write(self.root / "backend/config" / (name + ".json"), "[]")
        for name in ("TabProtocol", "TabBacking", "TabEconomics", "TabAgentToken", "TabLendingPool", "TabStockLending", "TabBuyback", "TabMarketHours"):
            self.write(self.root / "contracts/bnb/abi" / (name + ".json"), "[]")
        self.write(self.root / "contracts/deployments/bnb-56.json", json.dumps({"chain_id":56,"contracts":{"protocol":{"address":"0x1111111111111111111111111111111111111111"}}}))
        self.write(self.root / "README.md", "fixture project")
        self.write(self.root / ".env.example", "TAB_BNB_CHAIN_ID=56\n")
        data = self.root / "backend/data"
        data.mkdir(parents=True)
        for name, marker in (("tab.sqlite", "legacy account"), ("tab-solana.sqlite", "solana account"), ("custom-runtime.sqlite3", "custom account")):
            with sqlite3.connect(data / name) as db:
                db.execute("CREATE TABLE saved_accounts(owner TEXT)")
                db.execute("INSERT INTO saved_accounts VALUES(?)", (marker,))
        for command in ("curl", "systemctl", "install", "chown", "ln", "mv"):
            self.write(self.mock_bin / command, MOCK_COMMAND)
            (self.mock_bin / command).chmod(0o755)
        self.env = os.environ.copy()
        self.env["PATH"] = str(self.mock_bin) + os.pathsep + self.env.get("PATH", "/usr/bin:/bin")
        self.env["RELEASE_TEST_FIXTURE"] = json.dumps({"base":str(self.base),"root":str(self.root),"web":str(self.web),"unit":str(self.unit),"old_web":str(self.old_web)})

    @staticmethod
    def write(path, content):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    def rewrite(self, value):
        for actual, temporary in self.paths.items():
            value = value.replace(actual, temporary)
        return value

    @property
    def release(self):
        return self.root / "releases" / self.tag

    @property
    def web_release(self):
        return self.web / "releases" / self.tag

    def run_release(self, mode, failure="", timeout=15):
        env = dict(self.env, RELEASE_TEST_FAILURE=failure)
        return subprocess.run(["bash", str(self.script), mode, self.tag], env=env, capture_output=True, text=True, timeout=timeout)

    def stage(self):
        result = self.run_release("stage")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def commands(self):
        path = self.base / "commands.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def assert_original(self):
        self.assertEqual((self.web / "current").resolve(), self.old_web)
        self.assertEqual((self.root / "current-api").resolve(), self.old_api)
        self.assertEqual(self.unit.read_text(), self.legacy_unit)
        self.assertFalse((self.release / "published-at").exists())

    def test_stage_hashes_full_web_tree_and_service_configuration(self):
        self.stage()
        manifest = (self.release / "SHA256SUMS").read_text()
        for relative in ("assets/app.js", "assets/app.css", "assets/nested/lazy.js", "images/a-picture.svg", "media/demo.mp4"):
            self.assertIn(relative, manifest)
        self.assertIn("deploy/bnb.env", manifest)
        self.assertIn("deploy/tabagents-production.service", manifest)
        self.assertTrue((self.web_release / "index.html").is_file())

    def test_package_excludes_private_environment_databases_and_deploy_keypairs(self):
        self.write(self.root / "backend/.env", "credential-free forbidden fixture")
        self.write(self.root / "backend/config/private.env", "forbidden fixture")
        self.write(self.root / "backend/config/private.sqlite", "database fixture")
        self.write(self.root / "contracts/bnb/abi/private.key", "key fixture")
        self.write(self.root / "contracts/bnb/out/private-keypair.json", "keypair fixture")
        self.stage()
        manifest = (self.release / "SHA256SUMS").read_text()
        for forbidden in ("backend/.env", "private.env", "private.sqlite", "private.key", "private-keypair.json"):
            self.assertNotIn(forbidden, manifest)
        self.assertFalse((self.release / "backend/.env").exists())

    def test_public_launch_settings_override_operator_files_and_bound_provider_credits(self):
        self.stage()
        service = (self.release / "deploy/tabagents-production.service").read_text()
        environment_files = [line.partition("=")[2] for line in service.splitlines() if line.startswith("EnvironmentFile=")]
        self.assertEqual(environment_files[-1], str(self.root / "current-api/deploy/bnb.env"))
        self.assertIn("-" + str(self.operator / "bnb.env"), environment_files[:-1])
        self.assertIn("-" + str(self.operator / "sponsor.env"), environment_files[:-1])
        pins = (self.release / "deploy/bnb.env").read_text()
        self.assertIn("TAB_BNB_CHAIN_ID=56\n", pins)
        self.assertIn("TAB_BNB_SPONSOR_ADDRESS=0xA99Cf06fCdE993a6d2FaA73A2c82d67980Fd0416\n", pins)
        self.assertNotIn("TAB_BNB_SPONSOR_PRIVATE_KEY=", pins)
        self.assertNotIn("TAB_BNB_SPONSOR_ENABLED=", pins)
        self.assertIn("TAB_BNB_PROTOCOL=0x1111111111111111111111111111111111111111\n", pins)
        self.assertIn("TAB_OFFICIAL_TOKEN=\n", pins)
        self.assertIn("TAB_HOLDER_ACCESS_ENABLED=false\n", pins)
        self.assertIn("TAB_INFERENCE_DAILY_MICROS=100000\n", pins)
        self.assertNotIn("TAB_BNB_RPC=", pins)

    def configure_holder_fixture(self):
        path = self.root / "contracts/deployments/bnb-56.json"
        manifest = json.loads(path.read_text())
        manifest.update(official_tab_address="0xf07449517ae4b48808098c573a5347e67c714444", official_tab_code_hash="0x" + "12" * 32, holder_access_enabled=True)
        path.write_text(json.dumps(manifest))

    def test_official_tab_and_holder_policy_survive_release_packaging(self):
        self.configure_holder_fixture()
        self.stage()
        pins = (self.release / "deploy/bnb.env").read_text()
        self.assertIn("TAB_OFFICIAL_TOKEN=0xf07449517ae4b48808098c573a5347e67c714444\n", pins)
        self.assertIn("TAB_HOLDER_ACCESS_ENABLED=true\n", pins)
        self.assertEqual(pins.count("TAB_OFFICIAL_TOKEN="), 1)
        result = self.run_release("publish")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_holder_enforcement_mismatch_rolls_back_release(self):
        self.configure_holder_fixture()
        self.stage()
        result = self.run_release("publish", "holder_access")
        self.assertNotEqual(result.returncode, 0)
        self.assert_original()

    def test_cannot_package_holder_policy_without_verified_token(self):
        path = self.root / "contracts/deployments/bnb-56.json"
        manifest = json.loads(path.read_text())
        manifest["holder_access_enabled"] = True
        path.write_text(json.dumps(manifest))
        result = self.run_release("stage")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Holder access requires", result.stderr)

    def test_healthy_publish_switches_api_and_web_and_checks_entry_assets(self):
        self.stage()
        result = self.run_release("publish")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual((self.root / "current-api").resolve(), self.release)
        self.assertEqual((self.web / "current").resolve(), self.web_release)
        self.assertTrue((self.release / "published-at").is_file())
        urls = [arg for call in self.commands() if call["command"] == "curl" for arg in call["args"] if arg.startswith("https://")]
        self.assertIn("https://tabagents.io/assets/app.js", urls)
        self.assertIn("https://tabagents.io/assets/app.css", urls)

    def check_rollback(self, failure):
        self.stage()
        result = self.run_release("publish", failure)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assert_original()
        self.assertTrue((self.release / "previous-api.service").is_file())
        self.assertIn(["restart", "tabagents-api"], [call["args"] for call in self.commands() if call["command"] == "systemctl"])

    def test_startup_failure_rolls_back(self):
        self.check_rollback("startup")

    def test_wrong_api_config_rolls_back(self):
        self.check_rollback("config")

    def test_stale_public_index_rolls_back(self):
        self.check_rollback("public_index")

    def test_corrupt_public_bundle_rolls_back(self):
        self.check_rollback("public_asset")

    def test_missing_public_css_rolls_back(self):
        self.check_rollback("public_asset_http")

    def test_frontend_atomic_switch_failure_rolls_back(self):
        self.check_rollback("frontend_switch")

    def test_frontend_symlink_failure_rolls_back(self):
        self.check_rollback("frontend_link")

    def test_service_install_failure_rolls_back(self):
        self.check_rollback("service_install")

    def test_corrupt_staged_bundle_is_rejected_before_service_mutation(self):
        self.stage()
        (self.web_release / "assets/app.js").write_text("corrupted staged bundle")
        (self.base / "commands.jsonl").unlink()
        result = self.run_release("publish")
        self.assertNotEqual(result.returncode, 0)
        self.assert_original()
        self.assertFalse((self.release / "previous-api.service").exists())
        self.assertFalse(any(call["command"] == "systemctl" for call in self.commands()))

    def test_unlisted_environment_file_is_rejected_before_service_mutation(self):
        self.stage()
        (self.release / "unexpected.env").write_text("forbidden fixture content")
        (self.base / "commands.jsonl").unlink()
        result = self.run_release("publish")
        self.assertNotEqual(result.returncode, 0)
        self.assert_original()
        self.assertFalse((self.release / "previous-api.service").exists())
        self.assertFalse(any(call["command"] == "systemctl" for call in self.commands()))

    def test_symlinked_release_artifact_is_rejected_before_service_mutation(self):
        self.stage()
        path = self.release / "web/assets/app.js"
        path.unlink()
        path.symlink_to(self.root / "frontend/dist/assets/app.js")
        (self.base / "commands.jsonl").unlink()
        result = self.run_release("publish")
        self.assertNotEqual(result.returncode, 0)
        self.assert_original()
        self.assertFalse((self.release / "previous-api.service").exists())
        self.assertFalse(any(call["command"] == "systemctl" for call in self.commands()))

    def test_sqlite_snapshots_preserve_legacy_and_sol_account_rows(self):
        self.stage()
        live = sqlite3.connect(self.root / "backend/data/tab.sqlite")
        self.addCleanup(live.close)
        live.execute("PRAGMA journal_mode=WAL")
        live.execute("INSERT INTO saved_accounts VALUES('committed WAL account')")
        live.commit()
        self.assertTrue((self.root / "backend/data/tab.sqlite-wal").exists())
        result = self.run_release("publish")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for name, marker in (("tab.sqlite", "legacy account"), ("tab-solana.sqlite", "solana account"), ("custom-runtime.sqlite3", "custom account")):
            expected = [(marker,)] + ([("committed WAL account",)] if name == "tab.sqlite" else [])
            for directory in (self.root / "backend/data", self.release / "rollback-data"):
                path = directory / name
                with sqlite3.connect(path) as db:
                    self.assertEqual(db.execute("SELECT owner FROM saved_accounts").fetchall(), expected)
            self.assertEqual((self.release / "rollback-data" / name).stat().st_mode & 0o777, 0o600)

    def test_bnb_wal_family_is_owned_by_service_after_backup(self):
        self.stage()
        path = self.root / "backend/data/tab-bnb56.sqlite"
        live = sqlite3.connect(path)
        self.addCleanup(live.close)
        live.execute("PRAGMA journal_mode=WAL")
        live.execute("CREATE TABLE saved_accounts(owner TEXT)")
        live.execute("INSERT INTO saved_accounts VALUES('BNB draft')")
        live.commit()
        paths = [path, path.with_name(path.name + "-wal"), path.with_name(path.name + "-shm")]
        self.assertTrue(all(p.exists() for p in paths))
        result = self.run_release("publish")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        owned = {call["args"][-1] for call in self.commands() if call["command"] == "chown" and call["args"][0] == "ubuntu:ubuntu"}
        self.assertTrue({str(p) for p in paths}.issubset(owned))
        self.assertTrue(all(p.stat().st_mode & 0o777 == 0o600 for p in paths))
        self.assertEqual(live.execute("SELECT owner FROM saved_accounts").fetchall(), [("BNB draft",)])

    def test_symlinked_sqlite_journal_is_rejected_before_backup_and_switch(self):
        self.stage()
        sentinel = self.base / "private-sentinel"
        sentinel.write_text("unchanged")
        (self.root / "backend/data/tab.sqlite-shm").symlink_to(sentinel)
        result = self.run_release("publish")
        self.assertNotEqual(result.returncode, 0)
        self.assert_original()
        self.assertEqual(sentinel.read_text(), "unchanged")
        self.assertFalse(any(call["command"] == "systemctl" for call in self.commands()))

    def test_explicit_rollback_restores_the_saved_service_and_pointers(self):
        self.stage()
        published = self.run_release("publish")
        self.assertEqual(published.returncode, 0, published.stdout + published.stderr)
        result = self.run_release("rollback")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual((self.web / "current").resolve(), self.old_web)
        self.assertEqual((self.root / "current-api").resolve(), self.old_api)
        self.assertEqual(self.unit.read_text(), self.legacy_unit)

    def test_initial_migration_without_api_symlink_recovers_after_startup_failure(self):
        (self.root / "current-api").unlink()
        self.stage()
        result = self.run_release("publish", "startup")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "current-api").exists())
        self.assertFalse((self.root / "current-api").is_symlink())
        self.assertEqual(self.unit.read_text(), self.legacy_unit)
        self.assertEqual((self.web / "current").resolve(), self.old_web)
        self.assertEqual((self.release / "previous-api.path").read_text(), "\n")

    def test_initial_migration_manual_rollback_removes_the_new_api_symlink(self):
        (self.root / "current-api").unlink()
        self.stage()
        result = self.run_release("publish")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        result = self.run_release("rollback")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((self.root / "current-api").exists())
        self.assertFalse((self.root / "current-api").is_symlink())
        self.assertEqual(self.unit.read_text(), self.legacy_unit)
        self.assertEqual((self.web / "current").resolve(), self.old_web)

    def test_publisher_waits_or_refuses_when_host_lock_is_held(self):
        self.stage()
        lock = self.root / ".release.lock"
        with lock.open("a+") as handle:
            fcntl.flock(handle, fcntl.LOCK_EX)
            process = subprocess.Popen(["bash", str(self.script), "publish", self.tag], env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                time.sleep(0.8)
                self.assert_original()
                if process.poll() is not None:
                    self.assertNotEqual(process.returncode, 0, "Publisher ignored held lock")
                fcntl.flock(handle, fcntl.LOCK_UN)
                stdout, stderr = process.communicate(timeout=15)
                self.assertIn(process.returncode, (0, 1), stdout + stderr)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.communicate()

    def test_package_checksums_remain_valid_when_moved_to_a_different_root(self):
        packaged = self.run_release("package")
        self.assertEqual(packaged.returncode, 0, packaged.stdout + packaged.stderr)
        destination = self.base / "relocated"
        (destination / "releases").mkdir(parents=True)
        relocated = destination / "releases" / self.tag
        shutil.copytree(self.release, relocated)
        checked = subprocess.run(["sha256sum", "--quiet", "-c", "SHA256SUMS"], cwd=relocated, capture_output=True, text=True)
        self.assertEqual(checked.returncode, 0, checked.stdout + checked.stderr)


if __name__ == "__main__":
    unittest.main()
