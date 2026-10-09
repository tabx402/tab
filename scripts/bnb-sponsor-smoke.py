#!/usr/bin/env python3
"""Run the precompiled, ignored registration smoke with narrowly injected Vault keys.

Build tests without secrets first. This wrapper never compiles code and never
prints keys, signatures, or raw transactions. It does not fund either wallet.
"""
import argparse
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", type=Path, required=True)
    parser.add_argument("--authorize-mainnet-registration", action="store_true")
    args = parser.parse_args()
    if not args.authorize_mainnet_registration:
        parser.error("explicit mainnet registration authorization is required")
    binary = args.test_binary.resolve(strict=True)
    if binary.parent != ROOT / "backend/target/debug/deps" or not binary.name.startswith("tab_api-") or not os.access(binary, os.X_OK):
        parser.error("use the precompiled backend test executable under backend/target/debug/deps")
    for name in ("TAB_BNB_SPONSOR_KEY", "TAB_SPONSOR_SMOKE_OWNER_KEY"):
        if not os.environ.get(name):
            parser.error("inject both designated Vault aliases into this executable")
    os.umask(0o077)
    env = os.environ.copy()
    env["TAB_SPONSOR_SMOKE_AUTHORIZED"] = "chain56-register-only-max-0.00005-bnb"
    os.execve(binary, [str(binary), "sponsor_smoke::mainnet_zero_bnb_owner_registration", "--exact", "--ignored", "--nocapture", "--test-threads=1"], env)


if __name__ == "__main__":
    main()
