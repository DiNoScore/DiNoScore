let
  # Pin nixpkgs
  pkgs = import (builtins.fetchTarball {
    name = "nixpkgs-unstable-2022-04-21";
    url = "https://github.com/nixos/nixpkgs/archive/4c344da29a5b46caadb87df1d194082a190e1199.tar.gz";
    sha256 = "1m2m3wi52pr6gw5vg35zf3ykvp4ksllig5gdw6zvhk7i6v78ryci";
  }) { };
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
  ];

  buildInputs = [
    poppler
    poppler_data
    gtk4
    gdk-pixbuf
    atk
    libadwaita
    pango
    opencv
    portmidi
    stdenv.cc.cc.lib
    bzip2
    glib
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
