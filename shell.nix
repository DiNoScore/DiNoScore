let
  # Pin nixpkgs
  pkgs = import (builtins.fetchTarball {
    name = "nixpkgs-unstable-2021-05-11";
    url = "https://github.com/nixos/nixpkgs/archive/93123faae0281d2e97d12641a7cfad07c8028aff.tar.gz";
    sha256 = "0kc8rwsfsirr784hh2w143cy2yaqq7in7n5rzjx3j77z7nwsab26";
  }) { };
in with pkgs;
mkShell rec {
  buildInputs = [
    # Tools
    cargo
    curl.out
    lld

    # Native
    rustc
    pkg-config
    llvmPackages.clang
    llvmPackages.libclang
    (python39.withPackages (pypkgs: [
      pypkgs.pikepdf
    ]))

    # Dependencies
    poppler
    poppler_data
    gnome3.gtk3
    gdk-pixbuf
    atk
    libhandy
    pango
    opencv
    portmidi
    libtensorflow-bin
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
