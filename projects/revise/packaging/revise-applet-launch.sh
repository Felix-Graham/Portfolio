#!/usr/bin/env bash
# revise-applet-launch.sh — launches `revise-applet` the way
# `revise-launch.sh` launches `revise`, and for exactly the same reasons
# (read the comments there before changing anything here): the
# NixOS graphics-driver paths, and finding the binary without depending on
# whatever PATH happens to be launching a `.desktop` entry.
#
# It deliberately does not copy any of that logic. `revise-launch.sh`
# takes the name of the binary to find from `REVISE_BIN_NAME` (defaulting
# to `revise`), so this is only a pointer: same script, different binary.
# That keeps the two from drifting apart — a fix to the driver-path handling
# in one place fixes both windows.
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd -P)"
export REVISE_BIN_NAME=revise-applet

# Installed by packaging/install.sh, the two launchers sit side by side;
# from a source checkout they sit side by side in packaging/ too.
if [ -x "$SCRIPT_DIR/revise-launch.sh" ]; then
  exec "$SCRIPT_DIR/revise-launch.sh" "$@"
fi

# Last resort: maybe revise-launch.sh is on PATH under some setup this
# didn't anticipate.
if command -v revise-launch.sh >/dev/null 2>&1; then
  exec revise-launch.sh "$@"
fi

cat >&2 <<'MSG'
revise-applet-launch.sh: could not find revise-launch.sh next to this script.

Re-run packaging/install.sh, which installs both launchers together into
~/.local/bin.
MSG
exit 1
