{
  description = "Revise — a native Linux revision-planning app";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        nativeBuildInputs = with pkgs; [ pkg-config ];
        buildInputs = with pkgs; [
          wayland
          libxkbcommon
          libx11
          libxcursor
          libxrandr
          libxi
          libGL
        ];
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "revise";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          inherit buildInputs;
          # `wrapProgram` (from the very stable, long-standing
          # `makeWrapper` setup hook — deliberately *not*
          # `addOpenGLRunpath`, whose exact attribute name/location has
          # moved around between nixpkgs versions) here matters:
          # `libGL`/`wayland` above only get this binary the glvnd
          # *dispatch* stubs (`libGL.so`/`libEGL.so`) linked in — the
          # actual driver implementation (Mesa's, or a proprietary one
          # like NVIDIA's) is resolved separately, at runtime, and
          # lives under NixOS's `/run/opengl-driver`, a path that
          # changes depending on which driver `hardware.graphics`/
          # `hardware.nvidia` actually activated on a given machine.
          # Without this, `eframe`/`glutin` fails with
          # `NoGlutinConfigs` on any machine using a different (or
          # differently-versioned) driver than whatever `libGL` above
          # happened to resolve to at build time — wrapping the binary
          # to prepend that runtime path to `LD_LIBRARY_PATH` means it
          # picks up whichever driver is actually active on the
          # machine it's run on, not just whatever was present at
          # build time. Mirrors exactly what `revise-launch.sh` already
          # does at the shell-script level, just baked into the binary
          # itself here, so it's correct even launched some other way.
          nativeBuildInputs = nativeBuildInputs ++ [ pkgs.makeWrapper ];
          # Packaging: gives `nix build .#default` / `nix profile
          # install .` on their own — without the home-manager module
          # below — a proper icon and .desktop entry too, rather than
          # relying entirely on the home-manager path for desktop
          # integration. `packaging/revise.svg` is already part of
          # `src` (src = ./.  above copies the whole repo), so this
          # just installs it and a freshly generated .desktop file
          # (not `packaging/revise.desktop`, which is a
          # PATH-dependent template for the non-Nix `install.sh` path
          # instead — this one points `Exec=` straight at `$out/bin/
          # revise`, the same idiom the home-manager module's own
          # `desktopEntryText` already uses for its store-path
          # package) into the standard XDG locations under $out.
          postInstall = ''
            install -Dm644 packaging/revise.svg "$out/share/icons/hicolor/scalable/apps/revise.svg"
            mkdir -p "$out/share/applications"
            cat > "$out/share/applications/revise.desktop" <<DESKTOP
            [Desktop Entry]
            Type=Application
            Name=Revise
            Comment=A lightweight, native revision planner
            Exec=$out/bin/revise
            Icon=revise
            Terminal=false
            Categories=Education;Office;
            StartupNotify=true
            DESKTOP
          '';
          # Bug fix (found the hard way, running a real build): this
          # used to prepend only `/run/opengl-driver/lib` here, which
          # is all a `glutin`/EGL-based `eframe` (0.24, before the
          # NoGlutinConfigs-driven migration) ever actually needed at
          # runtime beyond what got linked in directly. Current
          # `eframe`'s `wgpu` backend also needs `libwayland-client.so`
          # itself resolvable at runtime — a real shared library from
          # the `wayland` package above, not a build-time-only header/
          # dispatch-stub situation like `libGL` — and NixOS never
          # exposes that on any global, well-known path the way
          # `/run/opengl-driver` exists for GPU drivers specifically.
          # `makeLibraryPath buildInputs` is exactly what
          # `devShells.default` below already uses for the same
          # reason; this was simply missing here. Without it: `Error:
          # WinitEventLoop(Os(OsError { ..., error:
          # WaylandError(Connection(NoWaylandLib)) }))` the moment the
          # wrapped binary is actually run, reproduced for real.
          postFixup = ''
            wrapProgram $out/bin/revise --prefix LD_LIBRARY_PATH : "/run/opengl-driver/lib:${pkgs.lib.makeLibraryPath buildInputs}"
            wrapProgram $out/bin/revise-reminderd --prefix LD_LIBRARY_PATH : "/run/opengl-driver/lib:${pkgs.lib.makeLibraryPath buildInputs}"
          '';
        };

        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          nativeBuildInputs = nativeBuildInputs ++ (with pkgs; [ cargo rustc rustfmt clippy rust-analyzer ]);
          # `/run/opengl-driver/lib` covers the same runtime-driver gap
          # as the `wrapProgram` call above, for anyone running
          # `cargo run` straight from this dev shell rather than
          # `nix build`.
          LD_LIBRARY_PATH = "/run/opengl-driver/lib:${pkgs.lib.makeLibraryPath buildInputs}";
        };

        formatter = pkgs.nixpkgs-fmt;
      })
    // {
      # Not per-system (a home-manager module is a plain function, not
      # a derivation) — merged in here alongside the eachDefaultSystem
      # outputs above rather than defined inside that call.
      #
      # This is the actual fix for the ".desktop `Exec=revise` doesn't
      # launch" issue: a bare `revise` only resolves if the binary
      # happens to already be on whatever PATH is launching it, which
      # was never guaranteed by anything in this repo. Importing this
      # module makes `programs.revise.enable = true;` install the
      # built package into `home.packages` (so `revise` genuinely is a
      # real command, usable from a terminal or `exec-once = revise`
      # in Hyprland's classic config) *and* points the generated
      # `.desktop` entry at that package's own store path directly, so
      # menu/autostart launching doesn't depend on PATH resolution at
      # all.
      homeManagerModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.programs.revise;
          revisePkg = self.packages.${pkgs.system}.default;
          desktopEntryText = ''
            [Desktop Entry]
            Type=Application
            Name=Revise
            Comment=A lightweight, native revision planner
            Exec=${revisePkg}/bin/revise
            Icon=revise
            Terminal=false
            Categories=Education;Office;
            StartupNotify=true
          '';
        in
        {
          options.programs.revise = {
            enable = lib.mkEnableOption "Revise, the native Linux revision-planning app";
            autostart =
              lib.mkEnableOption "launching Revise automatically at login, via XDG autostart";
            reminderDaemon.enable = lib.mkEnableOption ''
              revise-reminderd, a background daemon (feature request:
              "Background Notifications") that keeps checking for due
              reminders and event startup commands even after the Revise
              GUI itself has been closed
            '';
          };

          config = lib.mkIf cfg.enable {
            home.packages = [ revisePkg ];

            # Makes Revise show up in application launchers/menus,
            # `Exec`ing the store path rather than a bare command name.
            xdg.dataFile."applications/revise.desktop".text = desktopEntryText;

            # Opt-in: the same entry, copied into the XDG autostart
            # directory, so Revise also launches once at login —
            # works under Hyprland (via its XDG autostart support) and
            # most other desktop environments alike.
            xdg.configFile."autostart/revise.desktop" =
              lib.mkIf cfg.autostart { text = desktopEntryText; };

            # Feature request: "Background Notifications". A systemd
            # user unit is the natural fit for "keeps running after
            # the app is killed" — `revise-reminderd` itself already
            # defers to the GUI app whenever it's running (via
            # `gui_heartbeat`), so enabling this alongside actually
            # using the GUI day to day is safe, not an either/or
            # choice.
            systemd.user.services.revise-reminderd = lib.mkIf cfg.reminderDaemon.enable {
              Unit = {
                Description = "Revise background reminder daemon";
                After = [ "graphical-session.target" ];
                PartOf = [ "graphical-session.target" ];
              };
              Service = {
                ExecStart = "${revisePkg}/bin/revise-reminderd";
                Restart = "on-failure";
              };
              Install.WantedBy = [ "graphical-session.target" ];
            };
          };
        };
    };
}

