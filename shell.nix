let
  # Pin nixpkgs
  pkgs = import (builtins.fetchTarball {
    name = "nixpkgs-22.11-2022-11-21";
    url = "https://github.com/NixOS/nixpkgs/archive/192b2b780f32014a177a2bbed8569bee35ec2942.tar.gz";
    sha256 = "17mx9vg6r2azhvp43aan8bj7wg6siphjk37vs64c4lhizv2wqb4y";
  }) {};
in with pkgs;
mkShell rec {
  nativeBuildInputs = [
    # Tools
    cargo
    curl.out
    lld

    # Compiler
    rustc
    pkg-config
    llvmPackages.clang
    llvmPackages.libclang
    (python39.withPackages (pypkgs: [
      pypkgs.pikepdf
    ]))
    
    # Build dependencies
    gnome.adwaita-icon-theme
  ];

  buildInputs = let
    #glib = pkgs.enableDebugging (pkgs.glib.overrideAttrs (old: {
    #  dontStrip = true;
    #}));
    #gtk4 = enableDebugging ((pkgs.gtk4.override { inherit glib; }).overrideAttrs (old: {
    #  dontStrip = true;
    #}));
  in [
    poppler
    poppler_data
    gtk4
    glib
    gdk-pixbuf
    atk
    libadwaita
    pango
    opencv
    portmidi
    stdenv.cc.cc.lib
    bzip2
    cairo
  ];
  shellHook = ''
    export LD_LIBRARY_PATH="${lib.makeLibraryPath buildInputs}:''${LD_LIBRARY_PATH}";
    export LIBCLANG_PATH="${llvmPackages.libclang}/lib"
    export GDK_DPI_SCALE=1.3
    export RUST_BACKTRACE=1
    export CARGO_TARGET_DIR=./target
    export RUSTC_WRAPPER=
  '';
}
