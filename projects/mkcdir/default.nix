{ pkgs ? import <nixpkgs> {} }:

let
  mkcdir = pkgs.writeScriptBin "mkcdir" ''
    #!/usr/bin/env bash
    read DIR  
    mkdir "$DIR"
    cd "$DIR"
  '';
in

pkgs.mkShell {
  buildInputs = [
    pkgs.bash
    pkgs.git
    mkcdir
  ];

  shellHook = ''
    # mkcdir is now available in your PATH
  '';
}
