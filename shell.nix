let
  sources = import ./npins;
  pkgs = import sources.pkgs {};
in
with pkgs;
mkShell rec {
  nativeBuildInputs = [
    # Tools
    cargo
    curl.out
    lld
    npins

    # Compiler
    # rustc
    pkg-config
    llvmPackages.clang
    llvmPackages.libclang
    (python3.withPackages (pypkgs: [
      pypkgs.pikepdf
    ]))

    # Build dependencies
    adwaita-icon-theme

    # Test dependencies
    sway
    grim
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
    librsvg
    pango
    opencv
    portmidi
    stdenv.cc.cc.lib
    bzip2
    cairo
  ];
  shellHook = ''
    export LD_LIBRARY_PATH="${lib.makeLibraryPath buildInputs}''${LD_LIBRARY_PATH:+:''${LD_LIBRARY_PATH}}"
    export LIBCLANG_PATH="${llvmPackages.libclang}/lib"
    export GDK_DPI_SCALE=1.3
    export RUST_BACKTRACE=1
    export CARGO_TARGET_DIR=./target
    export RUSTC_WRAPPER=
  '';
}
