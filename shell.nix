let
  # Pin nixpkgs
  pkgs = import (builtins.fetchTarball {
    name = "nixpkgs-unstable-2021-01-04";
    url = "https://github.com/nixos/nixpkgs/archive/56bb1b0f7a33e5d487dc2bf2e846794f4dcb4d01.tar.gz";
    sha256 = "1wl5yglgj3ajbf2j4dzgsxmgz7iqydfs514w73fs9a6x253wzjbs";
  }) {};
in with pkgs;
mkShell rec {
  buildInputs = [
    cargo
    poppler
    poppler_data
    pkg-config
    gnome3.gtk3
    gdk-pixbuf
    atk
    libhandy
    pango
    opencv
    llvmPackages.clang
    llvmPackages.libclang
    portmidi
    libtensorflow-bin
    curl.out
    lld

    stdenv.cc.cc.lib
    bzip2
    glib
    cairo
  ];
  shellHook = ''
    export LD_LIBRARY_PATH="${stdenv.lib.makeLibraryPath buildInputs}:$${LD_LIBRARY_PATH}";
    export LIBCLANG_PATH="${llvmPackages.libclang}/lib"
    export GDK_DPI_SCALE=1.3
    export RUST_BACKTRACE=1
    export CARGO_TARGET_DIR=./target
    export RUSTC_WRAPPER=
  '';
}
