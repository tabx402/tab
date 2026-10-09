#!/usr/bin/env bash
set -euo pipefail
tag=${1:-}
if [[ ! "$tag" =~ ^[a-zA-Z0-9][a-zA-Z0-9._-]{1,79}$ ]]; then
  printf '%s\n' 'usage: scripts/publish-tab.sh RELEASE_TAG' >&2
  exit 2
fi
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
package="$root/prepared-releases/$tag"
if [[ ! -e "$package" ]]; then
  bash "$root/scripts/release-tab.sh" package "$tag" "$package"
fi
[[ -d "$package" && ! -L "$package" ]]
/usr/bin/python3 "$root/scripts/check-release.py" "$package"
# Only the explicitly packaged public artifacts are transferred. Runtime secrets,
# database files, wallet keypairs and the source checkout stay on their own hosts.
remote_upload=$(/usr/bin/python3 - <<'REMOTE'
import shlex
script = '''set -euo pipefail
root=/home/ubuntu/apps/tabagents
tag=$1
mkdir -p "$root/releases"
incoming=$(mktemp -d "$root/releases/.incoming-$tag.XXXXXXXX")
trap 'rm -rf -- "$incoming"' EXIT
tar -xzf - -C "$incoming" --no-same-owner
/usr/bin/python3 "$incoming/$tag/deploy/check-release.py" "$incoming/$tag"
exec 9> "$root/.release.lock"
flock -n 9 || { printf '%s\\n' 'Another Tab release operation is running.' >&2; exit 1; }
[[ ! -e "$root/releases/$tag" && ! -L "$root/releases/$tag" ]]
mv -T "$incoming/$tag" "$root/releases/$tag"
'''
print("bash -c " + shlex.quote(script) + " --")
REMOTE
)
tar -C "$root/prepared-releases" -czf - "$tag" | \
  ssh -o BatchMode=yes -o ConnectTimeout=12 tabagents-vps \
    "$remote_upload $tag"
ssh -o BatchMode=yes -o ConnectTimeout=12 tabagents-vps \
  "bash /home/ubuntu/apps/tabagents/releases/$tag/deploy/release-tab.sh stage-web $tag && bash /home/ubuntu/apps/tabagents/releases/$tag/deploy/release-tab.sh publish $tag"
