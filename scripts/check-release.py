#!/usr/bin/env python3
"""Verify a portable public release before transfer or activation."""
import hashlib
import pathlib
import re
import stat
import sys


def allowed(path: str) -> bool:
    fixed = {
        "README.md", ".env.example", "public-assets.paths", "WEB_SHA256SUMS",
        "bin/tab-api", "deploy/release-tab.sh", "deploy/check-release.py",
        "deploy/tabagents-production.service", "deploy/bnb.env",
    }
    if path in fixed:
        return True
    if re.fullmatch(r"backend/config/[A-Za-z0-9_-]+\.json", path):
        return True
    if path == "contracts/deployments/bnb-56.json":
        return True
    if re.fullmatch(r"contracts/bnb/abi/(TabProtocol|TabBacking|TabEconomics|TabAgentToken|TabLendingPool|TabStockLending|TabBuyback|TabMarketHours)\.json", path):
        return True
    if path.startswith("web/"):
        public = path[4:]
        return bool(re.fullmatch(r"[A-Za-z0-9_/-][A-Za-z0-9_./-]*\.(html|js|css|json|svg|png|webp|jpg|jpeg|ico|mp4|woff2?|txt|xml|webmanifest)", public)) and "keypair" not in public.lower()
    return False


def verify(root: pathlib.Path) -> None:
    if root.is_symlink() or not root.is_dir():
        raise ValueError("Release must be a regular directory.")
    files = set()
    for entry in root.rglob("*"):
        mode = entry.lstat().st_mode
        if stat.S_ISLNK(mode) or not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
            raise ValueError("Release links and special files are unsupported.")
        if stat.S_ISREG(mode):
            path = entry.relative_to(root).as_posix()
            if path == "SHA256SUMS":
                continue
            if not allowed(path):
                raise ValueError(f"Unsupported release path: {path}")
            files.add(path)
    hashes = {}
    for line in (root / "SHA256SUMS").read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64}) [ *]\./([A-Za-z0-9_./-]+)", line)
        if not match:
            raise ValueError("Invalid release checksum entry.")
        digest, path = match.groups()
        if pathlib.PurePosixPath(path).as_posix() != path or ".." in pathlib.PurePosixPath(path).parts or path in hashes:
            raise ValueError("Unsafe or duplicate checksum path.")
        hashes[path] = digest
    if hashes.keys() != files:
        raise ValueError("Release checksum coverage must match the complete file tree.")
    required = {
        "bin/tab-api", "web/index.html", "deploy/check-release.py",
        "deploy/release-tab.sh", "deploy/tabagents-production.service",
        "deploy/bnb.env", "WEB_SHA256SUMS", "public-assets.paths",
        "contracts/deployments/bnb-56.json",
        "contracts/bnb/abi/TabProtocol.json",
        "contracts/bnb/abi/TabBacking.json",
        "contracts/bnb/abi/TabEconomics.json",
    }
    if not required.issubset(files):
        raise ValueError("Required release artifacts are missing.")
    for path, expected in hashes.items():
        if hashlib.sha256((root / path).read_bytes()).hexdigest() != expected:
            raise ValueError(f"Release checksum mismatch: {path}")


if __name__ == "__main__":
    try:
        verify(pathlib.Path(sys.argv[1]))
    except (IndexError, OSError, ValueError) as error:
        print(f"Release rejected: {error}", file=sys.stderr)
        sys.exit(1)
