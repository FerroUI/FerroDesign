#!/bin/bash
# Builds the browser site of FerroDesign with FerroUI's script (FerroUI/scripts/build-browser.sh, which
# takes the same options) into target/browser/ferrodesign-app. The module draws with Vello alone: the
# build stops first when Skia is in the dependency graph of the browser target.
#
#   FerroUI/scripts/browser/setup.sh ~/browser-tools && source ~/browser-tools/env.sh
#   scripts/build-browser.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
"$ROOT/scripts/check-renderer.sh" wasm32-unknown-emscripten
FERROUI_BROWSER_WORKSPACE="$ROOT" "$ROOT/FerroUI/scripts/build-browser.sh" ferrodesign-app "$@"
