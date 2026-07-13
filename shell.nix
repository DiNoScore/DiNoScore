let
  sources = import ./npins;
  pkgs = import sources.pkgs {};
  pre-commit-check = (import sources.git-hooks).run {
    src = ./.;
    hooks.rustfmt.enable = true;
  };
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
    rustc
    pkg-config
    llvmPackages.clang
    llvmPackages.libclang
    (python3.withPackages (pypkgs: [
      pypkgs.pikepdf
    ]))

    # Build dependencies
    adwaita-icon-theme
    blueprint-compiler

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
    libseccomp
    glycin-loaders
    shared-mime-info # Glycin needs the MIME database to identify image bytes
  ];
  shellHook = ''
    ${pre-commit-check.shellHook}
    export LD_LIBRARY_PATH="${lib.makeLibraryPath buildInputs}''${LD_LIBRARY_PATH:+:''${LD_LIBRARY_PATH}}"
    export XDG_DATA_DIRS="${lib.makeSearchPath "share" buildInputs}''${XDG_DATA_DIRS:+:''${XDG_DATA_DIRS}}"
    export LIBCLANG_PATH="${llvmPackages.libclang.lib}/lib"
    export GDK_DPI_SCALE=1.3
    export RUST_BACKTRACE=1
    export CARGO_TARGET_DIR=./target
    export RUSTC_WRAPPER=
  '';
}
