#!/usr/bin/env bash
set -euo pipefail
mode=${1:-stage}
tag=${2:-}
if [[ ! "$tag" =~ ^[a-zA-Z0-9][a-zA-Z0-9._-]{1,79}$ ]]; then
  printf '%s\n' 'usage: scripts/release-tab.sh package|stage|stage-web|publish|rollback RELEASE_TAG' >&2
  exit 2
fi
root=/home/ubuntu/apps/tabagents
web=/var/www/tabagents
release="$root/releases/$tag"
if [[ "$mode" == package && -n "${3:-}" ]]; then
  release=$(realpath -m -- "$3")
fi
web_release="$web/releases/$tag"
if [[ "$mode" == stage || "$mode" == stage-web || "$mode" == publish || "$mode" == rollback ]]; then
  exec 9> "$root/.release.lock"
  flock -n 9 || { printf '%s\n' 'Another Tab release operation is running.' >&2; exit 1; }
fi
check_package() {
  /usr/bin/python3 "$release/deploy/check-release.py" "$release" || return 1
}
stage_web() {
  check_package || return 1
  [[ ! -e "$web_release" && ! -L "$web_release" ]] || return 1
  mkdir -p "$web/releases" || return 1
  mkdir "$web_release" || return 1
  cp -a "$release/web/." "$web_release/" || return 1
  (cd "$web_release" && sha256sum --quiet -c "$release/WEB_SHA256SUMS") || return 1
}
check_api() {
  local origin=$1
  curl --fail --silent --show-error --connect-timeout 10 --max-time 30 --retry 4 --retry-connrefused --retry-delay 1 "$origin/api/health" | /usr/bin/python3 -c 'import json,sys; v=json.load(sys.stdin); assert v.get("status")=="ok" and v.get("backend")=="rust"' || return 1
  curl --fail --silent --show-error --connect-timeout 10 --max-time 30 "$origin/api/config" | /usr/bin/python3 -c 'import json,sys; v=json.load(sys.stdin); assert v.get("backend")=="rust" and v.get("chain_id")==56 and v.get("usdt_address", "").lower()=="0x55d398326f99059ff775485246999027b3197955" and v.get("usdt_decimals")==18 and v.get("contracts_status")=="live" and v.get("financial_actions_enabled") is True' || return 1
  curl --fail --silent --show-error --connect-timeout 10 --max-time 30 "$origin/api/config" | /usr/bin/python3 -c 'import json,sys; v=json.load(sys.stdin); m=json.load(open(sys.argv[1])); assert v.get("official_tab_address")==m.get("official_tab_address"); assert v.get("holder_access_enabled",False) is m.get("holder_access_enabled",False)' "$release/contracts/deployments/bnb-56.json" || return 1
}
check_public_assets() {
  local asset expected actual
  while IFS= read -r asset; do
    expected=$(sha256sum "$web_release$asset" | cut -d ' ' -f 1) || return 1
    actual=$(curl --fail --silent --show-error --connect-timeout 10 --max-time 30 "https://tabagents.io$asset" | sha256sum | cut -d ' ' -f 1) || return 1
    [[ "$actual" == "$expected" ]] || return 1
  done < "$release/public-assets.paths"
}
restore_previous() {
  if [[ -n "$old_api" ]]; then
    ln -sfnT "$old_api" "$root/current-api.next" || return 1
    mv -Tf "$root/current-api.next" "$root/current-api" || return 1
  else
    rm -f "$root/current-api" || return 1
  fi
  install -m 644 "$release/previous-api.service" /etc/systemd/system/tabagents-api.service || return 1
  ln -sfnT "$old_web" "$web/current.next" || return 1
  mv -Tf "$web/current.next" "$web/current" || return 1
  systemctl daemon-reload || return 1
  systemctl restart tabagents-api || return 1
}
case "$mode" in
  package|stage)
    [[ -f "$root/backend/target/release/tab-api" ]]
    [[ -f "$root/frontend/dist/index.html" ]]
    [[ ! -e "$release" && ! -L "$release" ]]
    mkdir -p "$release/bin" "$release/backend/config" "$release/contracts/deployments" "$release/contracts/bnb/abi" "$release/deploy" "$release/web"
    binary="$root/backend/target/release/tab-api"
    install -m 755 "$binary" "$release/bin/tab-api"
    cp -a "$root/frontend/dist/." "$release/web/"
    shopt -s nullglob
    for config in "$root/backend/config/"*.json; do install -m 644 "$config" "$release/backend/config/"; done
    install -m 644 "$root/contracts/deployments/bnb-56.json" "$release/contracts/deployments/"
    for name in TabProtocol TabBacking TabEconomics TabAgentToken TabLendingPool TabStockLending TabBuyback TabMarketHours; do
      install -m 644 "$root/contracts/bnb/abi/$name.json" "$release/contracts/bnb/abi/"
    done
    shopt -u nullglob
    cp "$root/deploy/tabagents-production.service" "$release/deploy/"
    cp "$root/scripts/release-tab.sh" "$release/deploy/"
    cp "$root/scripts/check-release.py" "$release/deploy/"
    cat > "$release/deploy/bnb.env" <<'CONFIG'
TAB_PROJECT_ROOT=/home/ubuntu/apps/tabagents/current-api
TAB_HOST=127.0.0.1
TAB_PORT=4297
TAB_DATABASE=/home/ubuntu/apps/tabagents/backend/data/tab-bnb56.sqlite
TAB_BNB_CHAIN_ID=56
TAB_BNB_SPONSOR_ADDRESS=0xA99Cf06fCdE993a6d2FaA73A2c82d67980Fd0416
TAB_INFERENCE_DAILY_MICROS=100000
TAB_BNB_MANIFEST=/home/ubuntu/apps/tabagents/current-api/contracts/deployments/bnb-56.json
TAB_JOB_MERCHANTS=/home/ubuntu/apps/tabagents/current-api/backend/config/job-merchants-bnb.json
TAB_X402_MERCHANTS=/home/ubuntu/apps/tabagents/current-api/backend/config/x402-merchants-bnb.json
CONFIG
    /usr/bin/python3 - "$release" <<'CHAIN'
import json, pathlib, re, sys
root = pathlib.Path(sys.argv[1])
m = json.loads((root / "contracts/deployments/bnb-56.json").read_text())
a = m["contracts"]["protocol"]["address"]
if m.get("chain_id") != 56 or not re.fullmatch(r"0x[0-9a-fA-F]{40}", a):
    raise SystemExit("BNB deployment manifest is required.")
token = m.get("official_tab_address")
holders = m.get("holder_access_enabled", False)
if not isinstance(holders, bool):
    raise SystemExit("Holder access must be an explicit boolean.")
if token is not None and (not isinstance(token, str) or not re.fullmatch(r"0x[0-9a-f]{40}", token) or int(token, 16) == 0 or not re.fullmatch(r"0x[0-9a-f]{64}", str(m.get("official_tab_code_hash", "")))):
    raise SystemExit("The official TAB token and runtime hash must be verified.")
if holders and token is None:
    raise SystemExit("Holder access requires the official TAB token.")
with (root / "deploy/bnb.env").open("a") as env:
    env.write("TAB_BNB_PROTOCOL=" + a + "\n")
    env.write("TAB_OFFICIAL_TOKEN=" + (token or "") + "\n")
    env.write("TAB_HOLDER_ACCESS_ENABLED=" + str(holders).lower() + "\n")
CHAIN
    cp "$root/README.md" "$root/.env.example" "$release/"
    /usr/bin/python3 - "$release/web" "$release/public-assets.paths" <<'ASSETS'
import pathlib, re, sys
from html.parser import HTMLParser
web = pathlib.Path(sys.argv[1])
class Assets(HTMLParser):
    def __init__(self):
        super().__init__()
        self.paths = set()
    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        path = attrs.get("src") if tag == "script" else attrs.get("href") if tag == "link" else None
        if path and path.startswith("/assets/"):
            if not re.fullmatch(r"/assets/[A-Za-z0-9._/-]+", path) or ".." in pathlib.PurePosixPath(path).parts:
                raise ValueError("Unsafe public asset path.")
            if not (web / path.lstrip("/")).is_file():
                raise ValueError("Entry asset is missing.")
            self.paths.add(path)
parsed = Assets()
parsed.feed((web / "index.html").read_text())
if not any(path.endswith(".js") for path in parsed.paths):
    raise SystemExit("The public entry bundle is missing.")
if any(path.is_symlink() for path in web.rglob("*")):
    raise SystemExit("Public release symlinks are unsupported.")
pathlib.Path(sys.argv[2]).write_text("".join(path + "\n" for path in sorted(parsed.paths)))
ASSETS
    (cd "$release/web" && find . -type f -print0 | sort -z | xargs -0 sha256sum) > "$release/WEB_SHA256SUMS"
    (cd "$release" && find . -type f ! -path ./SHA256SUMS -print0 | sort -z | xargs -0 sha256sum) > "$release/SHA256SUMS"
    check_package
    if [[ "$mode" == stage ]]; then stage_web; fi
    printf '%s %s\n' "$mode" "$tag"
    ;;
  stage-web)
    stage_web
    printf 'staged web %s\n' "$tag"
    ;;
  publish)
    [[ -x "$release/bin/tab-api" && -f "$web_release/index.html" ]]
    [[ -f /etc/tabagents/api.env && -f "$release/deploy/bnb.env" ]]
    [[ ! -f "$release/previous-api.service" ]]
    [[ -L "$web/current" ]]
    for link in "$root/current-api" "$root/current-api.next" "$web/current.next"; do
      [[ ! -e "$link" || -L "$link" ]]
    done
    check_package
    (cd "$web_release" && sha256sum --quiet -c "$release/WEB_SHA256SUMS")
    expected_web=$(sha256sum "$web_release/index.html" | cut -d ' ' -f 1)
    install -d -m 700 -o ubuntu -g ubuntu "$root/backend/data" "$release/rollback-data"
    # The legacy tree stays in place for its saved service. Online backups retain
    # both databases without copying active WAL files or resetting account data.
    /usr/bin/python3 - "$root/backend/data" "$release/rollback-data" <<'BACKUP'
import os, pathlib, sqlite3, sys
source, dest = map(pathlib.Path, sys.argv[1:])
for path in sorted(set(source.glob("*.sqlite")) | set(source.glob("*.sqlite3")) | set(source.glob("*.db"))):
    if path.exists() or path.is_symlink():
        if path.is_symlink() or any(path.with_name(path.name + suffix).is_symlink() for suffix in ("-wal", "-shm")):
            raise SystemExit("Refusing a symlinked runtime database or journal.")
        with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True) as old, sqlite3.connect(dest / path.name) as snapshot:
            old.backup(snapshot)
        os.chmod(dest / path.name, 0o600)
BACKUP
    # Initialization or a read-only SQLite backup can create WAL sidecars as the
    # deploy user. The API must own the complete database family after backup.
    for database_path in "$root/backend/data/tab-bnb56.sqlite" "$root/backend/data/tab-bnb56.sqlite-wal" "$root/backend/data/tab-bnb56.sqlite-shm"; do
      [[ ! -L "$database_path" ]] || { printf '%s\n' 'Refusing a symlinked BNB database sidecar.' >&2; exit 1; }
      if [[ -f "$database_path" ]]; then
        chown ubuntu:ubuntu "$database_path"
        chmod 600 "$database_path"
      fi
    done
    old_web=$(readlink -f "$web/current")
    [[ -d "$old_web" ]]
    [[ -z "$old_web" ]] || ln -sfn "$old_web" "$web/previous"
    old_api=
    if [[ -L "$root/current-api" ]]; then
      old_api=$(readlink -f "$root/current-api")
      [[ -d "$old_api" ]]
    fi
    [[ -z "$old_api" ]] || ln -sfn "$old_api" "$root/previous-api"
    printf '%s\n' "$old_web" > "$release/previous-web.path"
    printf '%s\n' "$old_api" > "$release/previous-api.path"
    [[ -f "$release/deploy/tabagents-production.service" ]]
    cp /etc/systemd/system/tabagents-api.service "$release/previous-api.service"
    chmod 600 "$release/previous-api.service"
    switching=true
    trap 'rc=$?; if [[ "$switching" == true ]]; then restore_previous || printf "%s\n" "Rollback restart failed; recover using the saved service and release paths." >&2; fi; exit "$rc"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    if ! install -m 644 "$release/deploy/tabagents-production.service" /etc/systemd/system/tabagents-api.service \
      || ! ln -sfnT "$release" "$root/current-api.next" \
      || ! mv -Tf "$root/current-api.next" "$root/current-api" \
      || ! systemctl daemon-reload || ! systemctl restart tabagents-api || ! check_api http://127.0.0.1:4297; then
      printf '%s\n' 'API activation failed; restoring the previous service and frontend.' >&2
      exit 1
    fi
    if ! ln -sfnT "$web_release" "$web/current.next" \
      || ! mv -Tf "$web/current.next" "$web/current" \
      || ! check_api https://tabagents.io \
      || [[ "$(curl --fail --silent --show-error --connect-timeout 10 --max-time 30 -H 'Cache-Control: no-cache' https://tabagents.io/ | sha256sum | cut -d ' ' -f 1)" != "$expected_web" ]] \
      || ! check_public_assets; then
      printf '%s\n' 'Public verification failed; restoring the previous API and frontend.' >&2
      exit 1
    fi
    date -u +%Y-%m-%dT%H:%M:%SZ > "$release/published-at"
    switching=false
    trap - EXIT INT TERM
    printf 'published %s\n' "$tag"
    ;;
  rollback)
    [[ -f "$release/previous-api.service" ]]
    old_web=$(cat "$release/previous-web.path")
    [[ -d "$old_web" ]]
    old_api=$(cat "$release/previous-api.path")
    [[ -z "$old_api" || -d "$old_api" ]]
    restore_previous
    curl --fail --silent --show-error --connect-timeout 10 --max-time 30 http://127.0.0.1:4297/api/health
    ;;
  *) exit 2 ;;
esac
