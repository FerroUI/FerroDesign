#!/usr/bin/env bash
# One group of the browser scenarios (scripts/browser-ci-groups.json) in one rendering mode, against
# a built site: scripts/browser-group.sh <group> <Software2D|WebGL2> [site directory]
#
# The site is the third argument, else SITE (set by the browser tests of the shared FerroUI
# workflow), else target/browser/ferrodesign-app. The report goes to browser-report/<group>-<mode>.
set -euo pipefail

group="${1:?usage: browser-group.sh <group> <mode> [site]}"
mode="${2:?usage: browser-group.sh <group> <mode> [site]}"
root="$(cd "$(dirname "$0")/.." && pwd)"
site="${3:-${SITE:-$root/target/browser/ferrodesign-app}}"

scenarios="$(python3 - "$root/scripts/browser-ci-groups.json" "$group" <<'PY'
import json, sys
config = json.load(open(sys.argv[1]))
groups = {g["name"]: g["scenarios"] for g in config["groups"]}
if sys.argv[2] not in groups:
    sys.exit("unknown group %r; groups: %s" % (sys.argv[2], ", ".join(groups)))
print(",".join(groups[sys.argv[2]]))
PY
)"

cd "$root"
FERRODESIGN_DIST="$site" FERRODESIGN_MODES="$mode" FERRODESIGN_SCENARIOS="$scenarios" \
    FERRODESIGN_REPORT="$root/browser-report/$group-$mode" node tests/browser.mjs
