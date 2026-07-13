let
  sources = import ./npins;
  pkgs = import sources.pkgs { };
in
with pkgs;
rustPlatform.buildRustPackage rec {
  pname = "DiNoScore";
  version = "0.4.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./src
      ./res
      ./test
      ./.cargo
      ./Cargo.toml
      ./Cargo.lock
      ./build.rs
    ];
  };
  cargoLock = {
    lockFile = ./Cargo.lock;
    outputHashes = {
      "fern-0.6.0" = "sha256-7tyPLPgpNfM4z97vgYlWkNP18BQOoPZqVJbnDhhaR5s=";
      "pipeline-0.6.0" = "sha256-QBwWfynjcMDcfPC3dcWuWmXjPRlKW6Kpq9Q417PTcxI=";
      "xdg-2.5.2" = "sha256-4/lPsoyZseq2eg65J6FPY6uma1p2xrXCiGITQPdZ51Y=";
    };
  };
  doCheck = false;

  nativeBuildInputs = [
    pkg-config
    python3 # Pyo3 dependency
    glib # Gio resources
    adwaita-icon-theme # Icons
    blueprint-compiler # UI compilation
    llvmPackages.clang
  ];
  buildInputs = [
    openssl
    (python3.withPackages (pypkgs: [
      pypkgs.pikepdf
    ]))
    bzip2
    glib
    cairo
    atk
    libadwaita
    poppler
    poppler_data
    gtk4
    gdk-pixbuf
    librsvg
    pango
    opencv
    portmidi
    libseccomp
    glycin-loaders
  ];

  LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
}
