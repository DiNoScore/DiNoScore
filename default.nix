let
  sources = import ./npins;
  pkgs = import sources.pkgs { };
  pythonEnv = pkgs.python3.withPackages (pypkgs: [
    pypkgs.pikepdf
  ]);
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
    makeWrapper # Wrap binaries to expose pikepdf at runtime
  ];
  buildInputs = [
    openssl
    pythonEnv
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
  checkInputs = [
    shared-mime-info # Glycin needs the MIME database to identify image bytes
  ];

  LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";

  postInstall = ''
    mv $out/bin/viewer $out/bin/dinoscore
    mv $out/bin/editor $out/bin/dinoscore-editor
    mv $out/bin/cli $out/bin/dinoscore-cli

    install -Dm644 res/viewer/de.piegames.dinoscore.viewer.desktop \
      $out/share/applications/de.piegames.dinoscore.viewer.desktop
    install -Dm644 res/editor/de.piegames.dinoscore.editor.desktop \
      $out/share/applications/de.piegames.dinoscore.editor.desktop

    install -Dm644 res/de.piegames.dinoscore.svg \
      $out/share/icons/hicolor/scalable/apps/de.piegames.dinoscore.svg

    for prog in dinoscore dinoscore-editor dinoscore-cli; do
      wrapProgram $out/bin/$prog \
        --prefix PYTHONPATH : "${pythonEnv}/${pythonEnv.sitePackages}"
    done
  '';
}
