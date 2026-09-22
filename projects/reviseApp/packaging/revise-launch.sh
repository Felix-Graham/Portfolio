#!/usr/bin/env bash
# revise-launch.sh — the actual fix for ".desktop Exec=revise doesn't
# launch" (issues.md), for anyone NOT using the home-manager module
# in flake.nix (see packaging/README.md for that path instead).
#
# The root problem was never the .desktop file itself: `Exec=revise`
# only works if a `revise` binary happens to already be on whichever
# PATH is launching it, and nothing guaranteed that. This script is
# the one thing install.sh points every Exec= line at (a fixed,
# known, absolute path), and it does the PATH-independent part
# itself: search a handful of places a `revise` binary plausibly
# ended up, depending on how it was built or installed, and exec the
# first one found. That means the .desktop file never needs
# regenerating just because you later reinstalled Revise a different
# way (cargo install vs. a manual `cargo build --release` vs.
# whatever comes next).
set -euo pipefail

# Scope limitation, found the hard way running a real build on NixOS:
# this script only fixes the OpenGL *driver* path (below) — it
# deliberately can't do the same for general runtime libraries like
# `libwayland-client.so` (needed by `winit`, which `eframe` uses for
# windowing) the way `flake.nix`'s `nix build` path now does via
# `wrapProgram` + `makeLibraryPath`. On most distros that's fine:
# those libraries live at a normal, global system path and the
# dynamic linker just finds them, no script needed. On NixOS
# specifically, they don't — every library lives under a per-package
# `/nix/store/<hash>-<name>` path that only Nix itself knows, and a
# plain bash script has no portable way to discover it (short of
# shelling out to `nix` itself, adding a hard runtime dependency this
# script otherwise doesn't have). If you're on NixOS and hit
# `WaylandError(Connection(NoWaylandLib))` (or similar) running the
# binary this script launches: use `packaging/install.sh`'s preferred
# `nix build` path instead of its plain-`cargo build` fallback — that
# produces a binary with every library path already baked in, so this
# script's job (finding it, launching it) still applies, just against
# a binary that needs it less.

# NixOS-specific fixup for `NoGlutinConfigs`/`NoAvailablePixelFormat`-
# style crashes: `libGL.so`/`libEGL.so` (found fine via the ordinary
# dynamic-linking fixes elsewhere in this project) are just thin
# dispatch stubs (glvnd) — the *actual* driver implementation
# (Mesa's software/hardware renderer, or NVIDIA's proprietary one) is
# resolved separately, at runtime, via vendor JSON files. On NixOS,
# whichever driver `hardware.graphics`/`hardware.nvidia` actually
# activated lives under `/run/opengl-driver` (a stable path NixOS's
# system activation maintains specifically so apps don't need to know
# which GPU/driver is in use) — but nothing makes a plain `cargo
# build` (or even a `nix build` without nixpkgs' `addOpenGLRunpath`
# hook, see flake.nix) aware that path exists. Prepending it here
# covers both cases uniformly, regardless of how the binary itself
# was built. Harmless on non-NixOS systems: the path simply won't
# exist, so this is a no-op there.
if [ -d /run/opengl-driver/lib ]; then
  export LD_LIBRARY_PATH="/run/opengl-driver/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
if [ -d /run/opengl-driver-32/lib ]; then
  export LD_LIBRARY_PATH="/run/opengl-driver-32/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

# Belt-and-suspenders alongside the LD_LIBRARY_PATH fix above:
# `LD_LIBRARY_PATH` only helps once `libglvnd` (the thin `libEGL.so`/
# `libGL.so` dispatch layer) has already decided *which* vendor's
# actual driver library to `dlopen`. Deciding that in the first place
# is a separate lookup — glvnd reads vendor JSON files, normally from
# `/usr/share/glvnd/egl_vendor.d/` (a path that, like everything else
# under `/usr`, doesn't exist on NixOS) unless told otherwise via
# `__EGL_VENDOR_LIBRARY_DIRS`. NixOS's own aggregated driver directory
# carries these too, at `/run/opengl-driver/share/glvnd/egl_vendor.d`
# — exported here in case whichever `libglvnd` build ends up loaded
# doesn't already have this path patched in as one of its own
# defaults (nixpkgs' own `libglvnd` usually does, but a `libGL`/`mesa`
# story that differs slightly by nixpkgs version is exactly the kind
# of thing that's already bitten this project once — see flake.nix's
# `addOpenGLRunpath` history).
if [ -d /run/opengl-driver/share/glvnd/egl_vendor.d ]; then
  export __EGL_VENDOR_LIBRARY_DIRS="/run/opengl-driver/share/glvnd/egl_vendor.d${__EGL_VENDOR_LIBRARY_DIRS:+:$__EGL_VENDOR_LIBRARY_DIRS}"
fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." >/dev/null 2>&1 && pwd -P)"

CANDIDATES=(
  # Installed by packaging/install.sh, right next to this script.
  "$SCRIPT_DIR/revise"
  # Running straight from a source checkout without installing.
  "$REPO_ROOT/target/release/revise"
  "$REPO_ROOT/target/debug/revise"
  # `cargo install --path .` (or `cargo install revise`, if this ever
  # ends up published).
  "$HOME/.cargo/bin/revise"
  # Common manual/distro-packaging locations.
  "$HOME/.local/bin/revise"
  "/usr/local/bin/revise"
  "/usr/bin/revise"
)

for candidate in "${CANDIDATES[@]}"; do
  if [ -x "$candidate" ]; then
    exec "$candidate" "$@"
  fi
done

# Last resort: maybe it really is on PATH under some setup this list
# didn't anticipate.
if command -v revise >/dev/null 2>&1; then
  exec revise "$@"
fi

cat >&2 <<'EOF'
revise-launch.sh: could not find a `revise` binary anywhere searched.

Build it first, from the repo root:
    cargo build --release

Then either re-run packaging/install.sh, or place the resulting
target/release/revise binary in one of:
    ~/.local/bin/revise
    ~/.cargo/bin/revise
    /usr/local/bin/revise
EOF
exit 1
