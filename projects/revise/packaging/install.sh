#!/usr/bin/env bash
# packaging/install.sh — makes Revise a functional, launchable desktop
# app without needing Nix/home-manager (see packaging/README.md for
# that alternative path). Builds a release binary if one doesn't
# already exist, installs it plus the smart launcher
# (revise-launch.sh) to ~/.local/bin, and drops a .desktop entry into
# ~/.local/share/applications pointing at that launcher — so Revise
# shows up in application menus and, from a terminal, `revise-launch.sh`
# (or the real `revise` binary directly) both just work.
#
# Usage:
#   packaging/install.sh                 # install app + menu entry
#   packaging/install.sh --autostart     # also launch Revise at login
#                                         # (XDG autostart, works under
#                                         # Hyprland too)
#   packaging/install.sh --hyprland-exec-once
#                                         # also add an `exec-once`
#                                         # line to hyprland.conf, so
#                                         # Hyprland itself starts
#                                         # Revise at compositor
#                                         # startup — see the note on
#                                         # hyprctl vs exec-once below
#
# Both flags are opt-in and idempotent (safe to re-run) — nothing
# outside ~/.local and (with --hyprland-exec-once) one appended line
# in your own hyprland.conf is touched.
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." >/dev/null 2>&1 && pwd -P)"

BIN_DIR="$HOME/.local/bin"
APPS_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"
ICON_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"

AUTOSTART=false
HYPRLAND_EXEC_ONCE=false
for arg in "$@"; do
  case "$arg" in
    --autostart) AUTOSTART=true ;;
    --hyprland-exec-once) HYPRLAND_EXEC_ONCE=true ;;
    *)
      echo "install.sh: unknown option '$arg'" >&2
      exit 1
      ;;
  esac
done

echo "==> Building Revise..."
NIX_BUILD_USED=false
if command -v nix >/dev/null 2>&1 && [ -f "$REPO_ROOT/flake.nix" ]; then
  # Preferred path, and the actual fix for "the app doesn't launch
  # outside of its directory": a plain `cargo build` binary resolves
  # its graphics/Wayland shared libraries (libwayland-client,
  # libxkbcommon, libGL, ...) only via whatever `LD_LIBRARY_PATH` is
  # already set in the current shell — which on NixOS is normally
  # only true *inside* this repo's `nix develop` shell (or a directory
  # direnv auto-activates one for). Step outside that shell — a
  # different terminal, a `.desktop` launcher, systemd, a copy of the
  # binary elsewhere — and `LD_LIBRARY_PATH` is unset, so the dynamic
  # linker can't find those libraries at all, regardless of where the
  # binary itself lives.
  #
  # `nix build` sidesteps most of this: nixpkgs' `rustPlatform.
  # buildRustPackage` automatically patches in an RPATH for genuine
  # link-time dependencies. That alone still wasn't quite enough,
  # found the hard way, running a real build: `winit` (which `eframe`
  # uses for windowing) loads `libwayland-client.so` via `dlopen()` at
  # runtime rather than linking it at build time — invisible to
  # automatic RPATH patching, which only sees a binary's actual ELF
  # link-time dependencies. `flake.nix`'s own `postFixup` now also
  # explicitly wraps the binary with `LD_LIBRARY_PATH` covering every
  # `buildInputs` package (`wayland`, `libxkbcommon`, the X11 libs,
  # `libGL`), the same fix this comment already describes for the
  # OpenGL driver path specifically, just broadened — so the binary
  # this produces is genuinely portable now, launchable from anywhere,
  # by anything.
  echo "    (using 'nix build' — see packaging/README.md's"
  echo "    \"Why the binary might only launch from inside the repo\" for why"
  echo "    this matters more than it might seem)"
  GCROOT_DIR="$HOME/.local/state/revise"
  mkdir -p "$GCROOT_DIR"
  # A named, persistent out-link acts as a garbage-collection root —
  # without this, `nix store gc` (or the automatic collection this
  # system's `nix.settings.auto-optimise-store` can trigger) would be
  # free to delete the very store path this script is about to copy
  # the binary out of, the next time it runs.
  ( cd "$REPO_ROOT" && nix build .#default --out-link "$GCROOT_DIR/result" )
  REVISE_BIN="$GCROOT_DIR/result/bin/revise"
  REVISE_CLI_BIN="$GCROOT_DIR/result/bin/revise-cli"
  REVISE_APPLET_BIN="$GCROOT_DIR/result/bin/revise-applet"
  NIX_BUILD_USED=true
else
  echo "    'nix' not found (or no flake.nix here) — falling back to"
  echo "    'cargo build --release'."
  echo "    On NixOS specifically: the resulting binary may only launch"
  echo "    from inside a 'nix develop' shell (or wherever direnv"
  echo "    auto-activates one), for the reason explained in"
  echo "    packaging/README.md. Installing 'nix' (your configuration.nix"
  echo "    already enables flakes) and re-running this script will build"
  echo "    a properly portable binary instead."
  ( cd "$REPO_ROOT" && cargo build --release )
  REVISE_BIN="$REPO_ROOT/target/release/revise"
  REVISE_CLI_BIN="$REPO_ROOT/target/release/revise-cli"
  REVISE_APPLET_BIN="$REPO_ROOT/target/release/revise-applet"
fi

echo "==> Installing binary and launcher to $BIN_DIR"
mkdir -p "$BIN_DIR"
install -m 755 "$REVISE_BIN" "$BIN_DIR/revise"
install -m 755 "$SCRIPT_DIR/revise-launch.sh" "$BIN_DIR/revise-launch.sh"
# Direct request: "an API... callable from other applications and
# through the terminal from anywhere" — installed onto the same PATH
# as `revise` itself is the whole point, so `revise-cli register`/`log`
# work from any directory, exactly like `revise` already does.
install -m 755 "$REVISE_CLI_BIN" "$BIN_DIR/revise-cli"
# The small Timer / Pomodoro / Stopwatch window, and its launcher — a pointer
# at revise-launch.sh (see its own comments), so installed side by side.
install -m 755 "$REVISE_APPLET_BIN" "$BIN_DIR/revise-applet"
install -m 755 "$SCRIPT_DIR/revise-applet-launch.sh" "$BIN_DIR/revise-applet-launch.sh"

LAUNCHER="$BIN_DIR/revise-launch.sh"
APPLET_LAUNCHER="$BIN_DIR/revise-applet-launch.sh"

echo "==> Installing icon to $ICON_DIR"
mkdir -p "$ICON_DIR"
install -m 644 "$SCRIPT_DIR/revise.svg" "$ICON_DIR/revise.svg"
install -m 644 "$SCRIPT_DIR/revise-applet.svg" "$ICON_DIR/revise-applet.svg"

echo "==> Writing $APPS_DIR/revise.desktop"
mkdir -p "$APPS_DIR"
cat > "$APPS_DIR/revise.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Revise
Comment=A lightweight, native revision planner
Exec=$LAUNCHER
Icon=revise
Terminal=false
Categories=Education;Office;
StartupNotify=true
EOF

echo "==> Writing $APPS_DIR/revise-applet.desktop"
cat > "$APPS_DIR/revise-applet.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Revise Timer
GenericName=Timer, Pomodoro and Stopwatch
Comment=Revise's small timer, Pomodoro and stopwatch — logs your time to Revise
Exec=$APPLET_LAUNCHER
Icon=revise-applet
Terminal=false
Categories=Education;Utility;
Keywords=timer;pomodoro;stopwatch;focus;revise;
StartupNotify=true
StartupWMClass=revise-applet
EOF

if [ "$AUTOSTART" = true ]; then
  echo "==> Enabling XDG autostart (launch at login)"
  mkdir -p "$AUTOSTART_DIR"
  cp "$APPS_DIR/revise.desktop" "$AUTOSTART_DIR/revise.desktop"
fi

if [ "$HYPRLAND_EXEC_ONCE" = true ]; then
  HYPR_CONF="$HOME/.config/hypr/hyprland.conf"
  LINE="exec-once = $LAUNCHER"
  if [ ! -f "$HYPR_CONF" ]; then
    echo "==> No hyprland.conf found at $HYPR_CONF — skipping, add manually:"
    echo "    $LINE"
  elif grep -qF "$LAUNCHER" "$HYPR_CONF"; then
    echo "==> hyprland.conf already starts Revise — leaving it alone"
  else
    printf '\n# Added by revise/packaging/install.sh\n%s\n' "$LINE" >> "$HYPR_CONF"
    echo "==> Appended to $HYPR_CONF:"
    echo "    $LINE"
  fi
fi

# Most app launchers (GNOME Shell, KDE Plasma, most Wayland/X11 menus
# alike) watch ~/.local/share/{applications,icons} via inotify and
# pick up a new .desktop/icon immediately with no extra step — but
# not universally, and some caching layers (icon themes in
# particular) genuinely need an explicit rebuild rather than just a
# rescan. Both of the below are optional, standard, and silently
# skipped if not installed — this script works fine without either,
# just possibly with one extra login/logout before the icon and menu
# entry show up everywhere.
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -qtf "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true
fi

cat <<EOF

Done. Revise is installed:
  Binary:   $BIN_DIR/revise
  CLI:      $BIN_DIR/revise-cli (register/log external study time — see revise-cli sources)
  Launcher: $LAUNCHER
  Icon:     $ICON_DIR/revise.svg
  Menu entry: $APPS_DIR/revise.desktop
  Applet:   $BIN_DIR/revise-applet (launcher: $APPLET_LAUNCHER)
  Applet menu entry: $APPS_DIR/revise-applet.desktop
EOF

if [ "$NIX_BUILD_USED" = true ]; then
  cat <<EOF
  Built via: nix build (properly portable — see the note printed above)
EOF
else
  cat <<'EOF'

  Built via: cargo build --release (NOT via Nix). On NixOS, this
  binary depends on LD_LIBRARY_PATH being set the same way this
  repo's own `nix develop` shell sets it — it may fail to launch
  ("error while loading shared libraries: ...") from anywhere that
  isn't that shell. Re-run this script with `nix` available for a
  properly portable binary instead — see packaging/README.md.
EOF
fi

cat <<EOF

Run it now with: $LAUNCHER
EOF

# `~/.local/bin` is on PATH by default for bash/zsh on most distros
# (via /etc/profile.d or the shell's own XDG-aware defaults), but NOT
# for fish — fish never reads PATH from .profile-style files
# automatically, so `revise` resolving as a bare command needs one
# explicit, one-time step there. Detected via $SHELL rather than
# asking, since this is just an informational hint either way.
case "$(basename -- "${SHELL:-}")" in
  fish)
    echo "Your shell is fish, which won't put \$BIN_DIR on PATH automatically."
    echo "Run this once to fix 'fish: Unknown command: revise' for good:"
    echo "    fish_add_path $BIN_DIR"
    echo "(persists across sessions on its own — no need to edit config.fish)"
    ;;
  *)
    echo "(or plain 'revise' too, once a new shell picks up $BIN_DIR on PATH"
    echo "— most distros already put it there for bash/zsh; if 'revise' still"
    echo "isn't found, add 'export PATH=\"$BIN_DIR:\$PATH\"' to your shell's"
    echo "startup file.)"
    ;;
esac

cat <<EOF

For launching at Hyprland startup specifically: exec-once (used above
with --hyprland-exec-once) is Hyprland's own startup mechanism, run
directly by the compositor as it starts. hyprctl is a *runtime*
control tool — it can't fire "at startup" on its own since it talks to
an already-running Hyprland over its IPC socket, but it's the right
tool for launching Revise on demand later, e.g. bound to a keybind:
  bind = SUPER, R, exec, hyprctl dispatch exec $LAUNCHER
See packaging/README.md for the newer Lua-config equivalent
(hl.on("hyprland.start", ...)) too.
EOF
