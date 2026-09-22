# Packaging

## Notifications

Revise sends desktop notifications via `hyprctl notify` — Hyprland's
own built-in notification mechanism — rather than the freedesktop.org
D-Bus notification spec. This was a deliberate switch: the D-Bus route
(via the `notify-rust` crate) pulls in a real dependency on the system
D-Bus library, which on some setups — Nix in particular, where
`libdbus`/`dbus-1` aren't always straightforward to `nix search` for —
turns into a genuine build blocker for no benefit, since the plan's
target platform is specifically Hyprland anyway.

This means:
- No extra system packages or Nix inputs are needed for notifications
  to work.
- Notifications only work under Hyprland (or anything else that
  happens to implement the same `hyprctl notify` command) — running
  Revise under a different compositor or desktop environment, the
  reminder logic still runs correctly, but the notification call
  itself silently no-ops rather than displaying anything, since
  `hyprctl` won't be on `PATH`. That's consistent with how the rest of
  the app treats notification failures: swallowed, not surfaced,
  never something that interrupts the app.

## Installing `revise` as a real command (fixes `.desktop` `Exec=revise` not launching)

`packaging/revise.desktop`'s `Exec=revise` only works if a `revise`
binary is actually resolvable on `PATH` — which nothing in this repo
guaranteed on its own; installing the package with `nix build` alone
only puts it in `./result/bin/revise`, not on anyone's `PATH`. This is
the actual cause behind ".desktop file doesn't launch, `revise` isn't
a known command."

### `packaging/install.sh` (works with or without Nix; prefers Nix when available)

```sh
packaging/install.sh
```

Builds Revise — via `nix build` if `nix` is available (recommended;
see "Why the binary might only launch from inside the repo" below for
why this matters more than a normal Rust project), falling back to
`cargo build --release` otherwise — installs it plus a small launcher
script (`revise-launch.sh`, which finds the real binary regardless of
how it was built or installed — see its own comments) to
`~/.local/bin`, and writes a working `~/.local/share/applications/
revise.desktop` with an **absolute** `Exec=` path — so this isn't
dependent on `PATH` resolution at all, regardless of what shell or
session eventually launches it. Re-run any time to update after
pulling new changes; it's idempotent.

Two opt-in flags, covered in more detail under "Launching at startup"
below:

```sh
packaging/install.sh --autostart              # launch at login (XDG autostart)
packaging/install.sh --hyprland-exec-once     # launch at Hyprland startup specifically
```

One more thing this fixes: running plain `revise` from a terminal (as
opposed to the `.desktop` entry, which uses an absolute path and
doesn't care about `PATH` at all). `~/.local/bin` is on `PATH` by
default for bash/zsh on most distros, but **not for fish** — fish
never reads `PATH` from `.profile`-style files the way POSIX shells
do, so `fish: Unknown command: revise` after installing is expected,
not a bug. `install.sh` detects this via `$SHELL` and prints the exact
fix at the end (`fish_add_path ~/.local/bin`, which persists on its
own — no `config.fish` editing needed); bash/zsh get the equivalent
generic advice instead.

## Why the binary might only launch from inside the repo

**Diagnosing any native-window/graphics startup failure**: run with
`RUST_LOG=debug revise-launch.sh` — `eframe`/`glutin`'s own detailed
backend-selection tracing (which windowing/EGL backend was tried, how
many configs it found before filtering, etc.) is otherwise silently
discarded, since nothing consumes it by default.

**`NoGlutinConfigs` — KNOWN ISSUE, currently unresolved and parked.**
Reported on a real machine (NVIDIA proprietary driver 595.91.07,
Hyprland, Wayland): `eframe`'s `glow`/`glutin` (OpenGL/EGL) backend
fails outright with zero matching configs, before any window opens.
Ruled out so far, with real diagnostic data, not guesses: missing
libraries (confirmed present and correctly resolved — `/run/
opengl-driver/lib` has a complete, modern NVIDIA driver install),
missing `LD_LIBRARY_PATH`/RPATH (the `revise-launch.sh`/`wrapProgram`
fixes elsewhere in this file are real and necessary, just not
sufficient here), a Wayland-specific problem (fails identically under
forced X11), directory/shell-environment differences (a back-to-back
same-terminal test disproved this — an earlier apparent "it works
here" was a one-off, not reproducible), and the specific depth/
stencil size requested (`src/main.rs` sets `NativeOptions.depth_buffer
= 24` instead of `eframe`'s default of `0` — this was *initially
reported as fixing it*, but a same-session comparison later showed
the failure is identical either way; the setting is harmless and left
in place, but it does not fix this issue and should not be described
as doing so). Root cause is most likely a genuine version gap between
`glutin` (as vendored by `eframe = "0.24"`, from Dec 2023) and current
NVIDIA driver behavior — current `eframe` (0.36 at last check) defaults
to `wgpu` rendering instead, sidestepping `glutin`/EGL entirely, which
is the most promising lead but would require a real `eframe`/`egui`
version upgrade across this app's UI code — a substantial, higher-risk
change, deliberately not attempted yet. **Deliberately parked** rather
than pursued further for now, in favor of feature work. The de-risking
step this used to describe as a future TODO is now done:
`diagnostics/eframe-wgpu-smoke-test/` at the repo root is a minimal,
from-scratch current-`eframe` app (current version, `wgpu` explicitly
selected, `glow` explicitly excluded) for testing whether it actually
opens a window on the affected machine before attempting any real
migration — see that crate's own `README.md` for how to run it and
interpret the result, and `HANDOFF.md`'s "Known issues" section for
the fuller current state.

A different, more NixOS-specific version of the same underlying
problem as the `.desktop` issue above, and worth its own section since
it doesn't announce itself as clearly: "it launches fine when I run it
from the project directory, but not from a `.desktop` entry / a fresh
terminal / anywhere else" — often with an error like `error while
loading shared libraries: libwayland-client.so.0: cannot open shared
object file: No such file or directory` (naming whichever Wayland/
GL/xkbcommon library happens to come up first).

**Root cause:** a plain `cargo build`/`cargo build --release` binary
doesn't embed the location of its graphics/Wayland shared libraries
anywhere in itself — it relies entirely on the dynamic linker finding
them via `LD_LIBRARY_PATH` (or a handful of other runtime lookup
mechanisms) at the moment it's launched. On NixOS specifically, those
libraries don't live anywhere a generic Linux binary would think to
look (there's no `/usr/lib`), so something has to set
`LD_LIBRARY_PATH` explicitly. This repo's own `flake.nix` `devShell`
does exactly that (`LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath
buildInputs;`) — which is why the binary "just works" from inside a
`nix develop` shell (or a directory `direnv` auto-activates one for,
if `.envrc` contains `use flake` and `programs.direnv.enable` is
set — both are already true for anyone whose NixOS config resembles
the one this project has been developed against). Step outside that
shell and `LD_LIBRARY_PATH` reverts to whatever it normally is —
usually unset for these libraries — and the exact same binary, in the
exact same location, fails to launch.

**Diagnose it directly** rather than guessing: from a plain terminal
that has *not* run `nix develop` and is *not* inside the project
directory,

```sh
ldd ~/.local/bin/revise | grep "not found"
```

Anything listed is a library the dynamic linker couldn't locate in
that context — if this comes back empty but Revise still won't
launch, the problem is elsewhere (check `journalctl --user -xe` or
run it from a terminal to see the actual error, rather than however
it's currently being launched swallowing it).

**The fix, in order of how directly it addresses the root cause:**

1. **Build via `nix build`, not `cargo build`** — this is what
   `install.sh` now does automatically whenever `nix` is available (as
   of this project's `flake.nix`). `rustPlatform.buildRustPackage`'s
   own fixup phase computes and embeds the correct `RPATH` into the
   binary from its `buildInputs` (the same Wayland/GL/xkbcommon
   packages the `devShell` lists), so the built binary carries its own
   library paths and needs no ambient environment at all — no
   `LD_LIBRARY_PATH`, no `nix develop`, no `direnv`, nothing. This is
   the only one of these three fixes that makes the binary *actually*
   portable, as opposed to working around where it happens to be run
   from.
2. **`programs.nix-ld`** (already enabled, with a broad
   `nix-ld.libraries` list already covering `wayland`, `libGL`,
   `libxkbcommon`, `mesa`, `vulkan-loader`) is NixOS's general-purpose
   answer to "a foreign, non-Nix-built binary can't find its shared
   libraries" — but it only intervenes for binaries whose ELF
   interpreter is the generic `/lib64/ld-linux-x86-64.so.2` path,
   which is what upstream prebuilt binaries (AppImages, a `rustup`-
   installed toolchain's output, etc.) hardcode. A binary compiled
   with **nixpkgs' own `rustc`/`cargo`** (as opposed to `rustup`'s)
   already has its ELF interpreter set to a Nix store path, so
   `nix-ld` never gets involved for it either way — check with
   `patchelf --print-interpreter ~/.local/bin/revise` to see which
   case applies. If it's already the generic path and `nix-ld` still
   isn't covering it, the two most common reasons are (a) the session
   simply hasn't picked up the env vars yet — `programs.nix-ld`'s
   `NIX_LD`/`NIX_LD_LIBRARY_PATH` are set via `environment.variables`,
   which generally needs a fresh login (not just a new terminal) to
   take effect after first enabling it or changing its library list —
   or (b) a specific library `winit`'s Wayland backend needs isn't
   actually in the list despite looking like it should be (`ldd
   ... | grep "not found"` again is the way to find out which, if any).
3. **Manually exporting `LD_LIBRARY_PATH`** to match the `devShell`
   (e.g. wrapping the launcher script with `nix develop --command`, or
   hardcoding the same `nix.lib.makeLibraryPath` output into a
   wrapper) works, but is strictly worse than (1) — it's more to keep
   in sync by hand, and only "fixes" the one launch path it's applied
   to rather than the binary itself.

### A further wrinkle: `NoGlutinConfigs` even after the above

Fixing the shared-library-resolution problem above (via `nix build`,
`nix-ld`, or a manual `LD_LIBRARY_PATH`) is sometimes not quite the
end of it. If the binary now *launches* but immediately errors out
with something like:

```
Error: NoGlutinConfigs(ConfigTemplate { ... })
```

that's a **different** problem, one level down: `libGL.so`/`libEGL.so`
were found fine (so the app got as far as asking for a graphics
context) — but they're just thin dispatch stubs (`libglvnd`). The
*actual* driver implementation behind them — Mesa's software or Intel/
AMD renderer, or a proprietary driver like NVIDIA's — is resolved
separately, at runtime, via vendor JSON files, not via ordinary shared
library search paths. On NixOS, whichever driver `hardware.graphics`/
`hardware.nvidia` actually activated on a given machine lives under
`/run/opengl-driver` — a stable path NixOS's system activation
maintains specifically so applications don't need to hardcode which
GPU/driver is in use — but nothing about `RPATH`, `nix-ld`, or a
`devShell`'s `LD_LIBRARY_PATH` makes an app aware that path exists;
none of those are actually about *driver* resolution, only about
ordinary linked-library resolution.

Two fixes, both now applied:

- **`revise-launch.sh`** now prepends `/run/opengl-driver/lib` (and
  the 32-bit `/run/opengl-driver-32/lib`, if present) to
  `LD_LIBRARY_PATH` before `exec`ing the real binary, whenever that
  directory exists — a no-op on non-NixOS systems, since the path
  simply won't be there. This fixes it immediately, for a binary built
  either way (`nix build` or `cargo build`), without needing a
  rebuild.
- **`flake.nix`'s package derivation** now also wraps the built
  binary with `wrapProgram` (from nixpkgs' `makeWrapper` setup hook —
  a long-standing, very stable mechanism, deliberately *not*
  `addOpenGLRunpath`, whose exact attribute location has moved around
  between nixpkgs versions and errored with `attribute
  'addOpenGLRunpath' missing` on a recent nixpkgs revision when first
  tried) to prepend the same `/run/opengl-driver/lib` to
  `LD_LIBRARY_PATH`. This is the more "proper" fix for anyone building
  via `nix build`, since it means the binary works correctly even if
  launched some other way than through `revise-launch.sh` (e.g. run
  directly, or via a `.desktop` entry that doesn't go through the
  launcher for some reason).

Both do the same underlying thing (point the dynamic linker at
`/run/opengl-driver/lib`); keeping both means it's fixed regardless of
which of build method / launch method is actually being used on a
given machine.

**These driver-path fixes alone do not resolve `NoGlutinConfigs`** —
confirmed by the user hitting the exact same crash again, through
`revise-launch.sh`, with both fixes already in place. That's expected,
not a regression: the two fixes above address a *different* failure
mode (driver resolution) than the one actually happening here (see
`HANDOFF.md`'s "Known issues" section for the current, much more
specific theory — `eframe`'s `glutin`/EGL backend itself rejecting
every graphics config it's offered, unrelated to whether the driver
was found). `HANDOFF.md` also has the current recommended next step:
`diagnostics/eframe-wgpu-smoke-test/` at the repo root, a from-scratch
test of whether a current `eframe` (whose default backend isn't
`glutin` at all) fares any better on the same machine, before
attempting a real migration.

### Using home-manager instead

For anyone using home-manager as their primary way of managing dotfiles/packages, this flake exposes
a `homeManagerModules.default` module as a declarative alternative to
running `install.sh` by hand. Add it to your `flake.nix`'s
inputs/modules and enable it:

```nix
# flake.nix
{
  inputs.revise.url = "github:you/revise"; # or a local path: "path:/home/you/revise"
  # ...
  outputs = { self, home-manager, revise, ... }: {
    homeConfigurations.you = home-manager.lib.homeManagerConfiguration {
      # ...
      modules = [
        revise.homeManagerModules.default
        {
          programs.revise.enable = true;
          # Optional — also launches Revise once at login via XDG autostart:
          programs.revise.autostart = true;
        }
      ];
    };
  };
}
```

This does two things: it puts the built package on `home.packages`
(so `revise` is a genuinely real command from a terminal), and it
generates the `.desktop` entry itself pointing `Exec` directly at that
package's own Nix store path — the same "always use an absolute path,
never rely on PATH" fix `install.sh` applies for the non-Nix case
above, just derived from the Nix store instead of `~/.local/bin`.

Neither of the above? Install the package some other way that puts
`revise` on `PATH` (`nix profile install .`, `cargo install --path .`,
distro packaging, etc.) — the sections below assume one of the three
approaches has already been done.

## Launching at startup

Per Sprint 10's "application can launch at startup" criterion. Three
ways, from least to most Hyprland-specific:

### XDG autostart (works under most desktop environments, and under Hyprland too via its XDG autostart support)

The simplest option, and compositor-agnostic:

```sh
packaging/install.sh --autostart
```

Or, if using the home-manager module above, `programs.revise.autostart = true;`
does the same thing declaratively. Doing it fully by hand:

```sh
mkdir -p ~/.config/autostart
cp packaging/revise.desktop ~/.config/autostart/
```
(only after also arranging for `revise-launch.sh` to actually resolve — `install.sh` does the first `cp` for you with the right absolute path already filled in, which is why it's the recommended route.)

### Hyprland `exec-once` (Hyprland's own startup mechanism)

This is the actual "run this when Hyprland itself starts" hook —
executed directly by the compositor during its own startup sequence,
in its classic `.conf` config syntax:

```sh
packaging/install.sh --hyprland-exec-once
```

This idempotently appends a line like the below to
`~/.config/hypr/hyprland.conf` (skipping it if already present, and
never overwriting anything else in that file):

```
exec-once = /home/you/.local/bin/revise-launch.sh
```

Doing it by hand instead: add the same `exec-once = ` line yourself,
pointing at wherever `revise-launch.sh` (or the real `revise` binary
directly, if it's already on the `PATH` Hyprland launches with) ended
up.

### Hyprland's newer Lua config, and where `hyprctl` actually fits in

If using Hyprland's Lua configuration (`hl.*` API) rather than the
classic `.conf` syntax, the equivalent startup hook is:

```lua
hl.on("hyprland.start", function ()
  hl.exec_cmd("/home/you/.local/bin/revise-launch.sh")
  -- ...any other autostart commands already here...
end)
```

**A clarification on `hyprctl` specifically**, since it's easy to
conflate with the two mechanisms above: `hyprctl` is a *runtime*
control tool — it talks to an *already-running* Hyprland over its IPC
socket, so it has no way to fire "at startup" on its own before that
socket exists. `exec-once`/`hl.on("hyprland.start", ...)` are what
actually run something as Hyprland itself starts up. Where `hyprctl`
*is* the right tool: launching Revise on demand afterward, e.g. bound
to a keybind in `hyprland.conf`:

```
bind = SUPER, R, exec, hyprctl dispatch exec /home/you/.local/bin/revise-launch.sh
```

## Background Notifications (feature request)

`revise-reminderd` is a second, minimal binary built alongside the
main app (same crate — see `src/lib.rs`) that keeps polling for due
reminders and event startup commands after the Revise GUI has been
closed. It shares `notifications::check_reminders` directly with the
GUI, so there's no separate reminder logic to keep in sync between
the two.

The GUI app and the daemon coordinate automatically so they don't
double up: the GUI leaves a small heartbeat file behind
(`$XDG_RUNTIME_DIR/revise-gui.heartbeat`) every time it scans, and the
daemon checks that before every scan of its own, deferring to the GUI
whenever it looks like the GUI is already running. Practically: it's
safe to enable the daemon permanently and just use the GUI normally —
the daemon effectively only does anything while the GUI is closed.

**Recommended (home-manager):**

```nix
programs.revise.reminderDaemon.enable = true;
```

(alongside `programs.revise.enable = true;` from the section above —
this only adds a `systemd.user.services.revise-reminderd` unit, it
doesn't install/enable Revise itself on its own).

**Manual (no home-manager):** copy the provided unit and enable it
with `systemctl --user`:

```sh
mkdir -p ~/.config/systemd/user
cp packaging/revise-reminderd.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now revise-reminderd.service
```

Edit that file's `ExecStart=` to a full path first if
`revise-reminderd` isn't resolvable on the `PATH` systemd's user
manager runs with (same caveat as the `.desktop` `Exec=` line above).

## App icon

`packaging/revise.svg` — a simple icon matching the app's own
monochrome palette (`theme.rs`'s `Palette::dark`: `#0A0A0A`
background, `#F2F2F2` glyph), rather than introducing an accent color
used nowhere else in the app. Fixes a real gap: `Icon=revise` in
`packaging/revise.desktop` (and the home-manager module's own
generated entry) previously resolved to nothing on disk anywhere, so
every install path showed a blank/generic icon regardless of how
correctly everything else was set up.

Installed to the standard XDG hicolor theme location
(`~/.local/share/icons/hicolor/scalable/apps/revise.svg`) by
`packaging/install.sh`, which also calls `update-desktop-database`
and `gtk-update-icon-cache` afterward if either is present (both
optional, silently skipped otherwise — most launchers pick up a new
`.desktop`/icon via inotify without needing this, but not
universally). The Nix package (`packages.default` in `flake.nix`)
installs the same icon plus its own freshly generated `.desktop` entry
(`Exec=$out/bin/revise`, not the `install.sh`-oriented template file)
into `$out/share/...`, so `nix build .#default` / `nix profile install
.` alone — without the home-manager module — also gets full desktop
integration.

One Nix-specific gotcha worth flagging: flakes only see files tracked
by git. If `packaging/revise.svg` (or any other new file) isn't
`git add`ed yet, `src = ./.;` in `flake.nix` won't include it, and
`nix build` will fail on the `install -Dm644 packaging/revise.svg ...`
line in `postInstall` with a plain "no such file" error that has
nothing to do with the icon itself being wrong — `git add
packaging/revise.svg` (even uncommitted, just staged, is enough) fixes
it.

## Launching from the application menu

Already handled by `packaging/install.sh` (or the home-manager module,
if using that instead). Doing it fully by hand:

```sh
mkdir -p ~/.local/share/applications
cp packaging/revise.desktop ~/.local/share/applications/
```

(The same file works for both this and autostart above — copy it to
one directory, the other, or both, depending on what's wanted. Either
way, make sure `revise-launch.sh` actually resolves — see "Installing
`revise` as a real command" above — or edit the `Exec=` line to its
absolute path, which is exactly what `install.sh` does for you.)

## What hasn't been verified

This was built and tested in a sandboxed environment with no display
server, no Hyprland instance, and no working `nix` evaluator — so the
following are implemented against the relevant Hyprland/XDG/
home-manager mechanisms but **not verified against a real
compositor or a real `nix flake check`/`home-manager switch`**:

- Actual on-screen rendering under Hyprland specifically (the
  Wayland backend is enabled and the app runs under generic Wayland
  compositors during development, but Hyprland's specific window
  rules, workspace behavior, etc. haven't been exercised).
- `hyprctl notify` actually producing an on-screen notification —
  the command is built to match the same syntax used elsewhere in a
  working Hyprland config (`hyprctl notify <icon> <time_ms> <color>
  <message>`), but there's no running Hyprland instance here to
  confirm one actually appears.
- The autostart snippet being picked up correctly by a real Hyprland
  session, or the `.desktop` file being picked up correctly by a real
  application menu.
- The `homeManagerModules.default` module itself — written against
  the standard `home.packages`/`xdg.dataFile`/`xdg.configFile`/
  `systemd.user.services` home-manager API, but never evaluated with
  `home-manager switch` (or even `nix flake check`) against a real
  home-manager instance in this sandbox, since neither `nix` nor
  `home-manager` are available here.
- `revise-reminderd` itself runs and exits cleanly in this sandbox
  (`cargo run --bin revise-reminderd`), and its GUI/daemon
  heartbeat-based coordination is unit-tested
  (`gui_heartbeat::tests`), but the actual systemd unit — whether
  `systemctl --user enable --now` picks it up correctly, restarts it
  as expected, etc. — hasn't been exercised against a real systemd
  user instance, since this sandbox has none.
- `packaging/install.sh` and `packaging/revise-launch.sh` **were
  actually run end to end in this sandbox** (a real `cargo build
  --release`, not faked — `nix` genuinely isn't on `PATH` here, so
  this exercised the same fallback branch a non-Nix user would hit):
  binary and launcher installed, icon installed to the real XDG
  hicolor path, the generated `.desktop` file passed
  `desktop-file-validate` cleanly, and `revise-launch.sh` correctly
  located and exec'd the installed binary — which then got as far as
  `winit` trying to open a display and failing only because this
  sandbox genuinely has no Wayland/X11 server at all
  (`XdgRuntimeDirNotSet` / `XOpenDisplayFailed`), which is the
  expected, correct failure mode here rather than a packaging bug.
  `--autostart` and `--hyprland-exec-once` were both exercised too
  (against a real `~/.config/autostart/` and a fake
  `~/.config/hypr/hyprland.conf`), including confirming a second run
  doesn't duplicate the appended `exec-once` line. Still not
  exercised: a real Hyprland session actually reading either, or a
  real application menu rendering the new icon and entry.
- The Nix package's own `postInstall` (installing the icon and a
  generated `.desktop` file into `$out/share/...`, added alongside the
  icon above) is new, untested code with the same limitation as
  everything else `nix`-related in this list: no `nix` evaluator is
  available in this sandbox to run it at all, faked or otherwise.
- `install.sh`'s `nix build` branch (added specifically to fix the
  "only launches from inside the repo" `LD_LIBRARY_PATH` issue) was
  exercised in this sandbox only against a **fake `nix` executable**
  standing in for a real build — `nix` itself isn't installable here.
  What's confirmed: the branching logic picks `nix build` when `nix`
  is on `PATH` and falls back to `cargo build --release` correctly
  when it isn't, the `--out-link` gc-root path is created and used
  correctly, and the final installed binary/desktop entry come out
  identical either way. **Not confirmed**: that a *real* `nix build
  .#default` actually produces a binary with a correctly-embedded
  RPATH covering every Wayland/GL library `winit`/`glow` need at
  runtime — that depends on nixpkgs' own `buildRustPackage` fixup
  behavior working as expected for this specific dependency set,
  which needs a real NixOS machine (or at least a real Nix install) to
  verify.
- The `wrapProgram` fixup in `flake.nix` and the `/run/opengl-driver`
  prepend in `revise-launch.sh` (added to fix a reported
  `NoGlutinConfigs` crash) are both written against stable,
  well-documented mechanisms, but **neither has been exercised against
  a real GPU/driver at all** — this sandbox has no GPU, no
  `/run/opengl-driver`, and no `nix` to actually run a real build
  through. (An earlier version of the `flake.nix` fix used
  `addOpenGLRunpath` directly instead of `wrapProgram` — this was
  caught immediately on the first real build against actual NixOS,
  which failed with `attribute 'addOpenGLRunpath' missing` on that
  machine's pinned nixpkgs revision. `wrapProgram`/`makeWrapper` was
  chosen as the replacement specifically for being a far more stable,
  long-standing nixpkgs mechanism than a specific internal attribute
  path that's evidently moved between nixpkgs versions — but it's
  still only been confirmed to *evaluate*, via `nix build`'s dry-run
  behavior mentioned above with the fake `nix` shim, not to actually
  *produce a working binary*.) If `revise` still
  fails to find a usable graphics config after both fixes, the next
  things to check are `hardware.nvidia.package` (whether it's pinned
  to a version compatible with the running kernel) and whether
  `nvidia-drm.modeset=1` actually took effect (`cat
  /sys/module/nvidia_drm/parameters/modeset` should read `Y`) — both
  are NVIDIA/Wayland driver-level concerns outside what packaging
  alone can fix.

If something doesn't work as expected on real hardware, these are the
places to look first.

