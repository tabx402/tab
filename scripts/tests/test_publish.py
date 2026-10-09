"""Exercise the real publisher with SSH replaced by isolated local processes.

No sockets, live services, credentials or project release directories are used.
Run with: python3 -m unittest discover -s scripts/tests -p 'test_publish.py' -v
"""

import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


PROJECT = Path(__file__).resolve().parents[2]
REMOTE_PROJECT = "/home/ubuntu/apps/tabagents"

MOCK_SSH = r'''#!/usr/bin/python3
import json, os, pathlib, shlex, subprocess, sys

base = pathlib.Path(os.environ["PUBLISH_TEST_BASE"])
remote = pathlib.Path(os.environ["PUBLISH_TEST_REMOTE"])
args = sys.argv[1:]
assert args[:5] == ["-o", "BatchMode=yes", "-o", "ConnectTimeout=12", "tabagents-vps"], args
assert len(args) == 6, args
command = args[-1]
decoded = shlex.split(command)
upload = decoded[:2] == ["bash", "-c"]
with (base / "ssh.jsonl").open("a") as log:
    log.write(json.dumps({"upload": upload, "command": command}) + "\n")
if upload:
    assert len(decoded) == 5 and decoded[3] == "--", decoded
    script = decoded[2].replace("/home/ubuntu/apps/tabagents", str(remote))
    payload = sys.stdin.buffer.read()
    if os.environ.get("PUBLISH_TEST_FAILURE") == "transfer":
        # A truncated archive makes the real remote tar fail after creating its
        # incoming directory, exercising its EXIT cleanup rather than a stub.
        payload = payload[:len(payload) // 2]
    result = subprocess.run(["bash", "-c", script, "--", decoded[4]], input=payload)
else:
    assert command.startswith("bash /home/ubuntu/apps/tabagents/releases/"), command
    script = command.replace("/home/ubuntu/apps/tabagents", str(remote))
    result = subprocess.run(["bash", "-c", script], stdin=subprocess.DEVNULL)
sys.exit(result.returncode)
'''

MOCK_ACTIVATION = r'''#!/usr/bin/env bash
set -euo pipefail
/usr/bin/python3 - "$1" "$2" <<'CHECK'
import json, os, pathlib, subprocess, sys
mode, tag = sys.argv[1:]
assert mode in {"stage-web", "publish"}, mode
base = pathlib.Path(os.environ["PUBLISH_TEST_BASE"])
release = pathlib.Path(os.environ["PUBLISH_TEST_REMOTE"]) / "releases" / tag
subprocess.run(["/usr/bin/python3", str(release / "deploy/check-release.py"), str(release)], check=True)
with (base / "activation.jsonl").open("a") as log:
    log.write(json.dumps({"mode": mode, "tag": tag, "verified_final": release.is_dir()}) + "\n")
CHECK
'''


class PublishDriverTests(unittest.TestCase):
    tag = "candidate-01"

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="tab-publish-driver-")
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name)
        self.client = self.base / "client"
        self.remote = self.base / "host"
        self.mock_bin = self.base / "mock-bin"
        self.package = self.client / "prepared-releases" / self.tag
        self.final = self.remote / "releases" / self.tag
        for directory in (self.client / "scripts", self.remote, self.mock_bin):
            directory.mkdir(parents=True)
        shutil.copy2(PROJECT / "scripts/publish-tab.sh", self.client / "scripts/publish-tab.sh")
        shutil.copy2(PROJECT / "scripts/check-release.py", self.client / "scripts/check-release.py")
        self.write(self.mock_bin / "ssh", MOCK_SSH, executable=True)

        artifacts = {
            "README.md": "public fixture release\n",
            ".env.example": "TAB_BNB_CHAIN_ID=56\n",
            "bin/tab-api": "compiled public fixture\n",
            "web/index.html": '<!doctype html><script src="/assets/app.js"></script>',
            "web/assets/app.js": "console.log('BNB USDT fixture');\n",
            "deploy/release-tab.sh": MOCK_ACTIVATION,
            "deploy/check-release.py": (PROJECT / "scripts/check-release.py").read_text(),
            "deploy/tabagents-production.service": "[Service]\nUser=ubuntu\n",
            "deploy/bnb.env": "TAB_BNB_CHAIN_ID=56\n",
            "public-assets.paths": "/assets/app.js\n",
            "contracts/deployments/bnb-56.json": "{}",
            "contracts/bnb/abi/TabProtocol.json": "[]",
            "contracts/bnb/abi/TabBacking.json": "[]",
            "contracts/bnb/abi/TabEconomics.json": "[]",
        }
        for name, content in artifacts.items():
            self.write(self.package / name, content)
        web_checksums = self.checksum_lines(self.package / "web")
        self.write(self.package / "WEB_SHA256SUMS", web_checksums)
        self.write(self.package / "SHA256SUMS", self.checksum_lines(self.package))
        self.env = dict(
            os.environ,
            PATH=str(self.mock_bin) + os.pathsep + os.environ.get("PATH", "/usr/bin:/bin"),
            PUBLISH_TEST_BASE=str(self.base),
            PUBLISH_TEST_REMOTE=str(self.remote),
        )

    @staticmethod
    def write(path, text, executable=False):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        if executable:
            path.chmod(0o755)

    @staticmethod
    def checksum_lines(root):
        return "".join(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  ./{path.relative_to(root).as_posix()}\n"
            for path in sorted(root.rglob("*"))
            if path.is_file() and path.name != "SHA256SUMS"
        )

    def run_driver(self, failure=""):
        return subprocess.run(
            ["bash", str(self.client / "scripts/publish-tab.sh"), self.tag],
            env=dict(self.env, PUBLISH_TEST_FAILURE=failure),
            capture_output=True,
            text=True,
            timeout=15,
        )

    def log(self, name):
        path = self.base / name
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def assert_no_activation(self):
        self.assertEqual(self.log("activation.jsonl"), [])

    def assert_incoming_cleaned(self):
        self.assertEqual(list((self.remote / "releases").glob(".incoming-*")), [])

    def test_verified_transfer_promotes_complete_package_then_activates(self):
        result = self.run_driver()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue(self.final.is_dir())
        source = {str(path.relative_to(self.package)): path.read_bytes() for path in self.package.rglob("*") if path.is_file()}
        target = {str(path.relative_to(self.final)): path.read_bytes() for path in self.final.rglob("*") if path.is_file()}
        self.assertEqual(target, source)
        self.assertEqual([call["upload"] for call in self.log("ssh.jsonl")], [True, False])
        self.assertEqual(self.log("activation.jsonl"), [
            {"mode": "stage-web", "tag": self.tag, "verified_final": True},
            {"mode": "publish", "tag": self.tag, "verified_final": True},
        ])
        self.assert_incoming_cleaned()

    def test_truncated_transfer_cleans_incoming_and_never_promotes(self):
        result = self.run_driver("transfer")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.final.exists())
        self.assertEqual(len(self.log("ssh.jsonl")), 1)
        self.assert_no_activation()
        self.assert_incoming_cleaned()

    def test_existing_final_is_preserved_without_activation(self):
        self.write(self.final / "sentinel.txt", "existing immutable release\n")
        result = self.run_driver()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(list(self.final.iterdir()), [self.final / "sentinel.txt"])
        self.assertEqual((self.final / "sentinel.txt").read_text(), "existing immutable release\n")
        self.assert_no_activation()
        self.assert_incoming_cleaned()

    def test_unlisted_env_is_rejected_before_any_ssh(self):
        self.write(self.package / ".env", "SYNTHETIC_TEST_VALUE=not-a-secret\n")
        result = self.run_driver()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Unsupported release path: .env", result.stderr)
        self.assertEqual(self.log("ssh.jsonl"), [])
        self.assertFalse(self.final.exists())
        self.assert_no_activation()

    def test_host_lock_refuses_promotion_and_cleans_incoming(self):
        with (self.remote / ".release.lock").open("a+") as handle:
            fcntl.flock(handle, fcntl.LOCK_EX)
            result = self.run_driver()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Another Tab release operation is running.", result.stderr)
        self.assertFalse(self.final.exists())
        self.assert_no_activation()
        self.assert_incoming_cleaned()


if __name__ == "__main__":
    unittest.main()
