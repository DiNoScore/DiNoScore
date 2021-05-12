let
  # Pin nixpkgs
  pkgs = import (builtins.fetchTarball {
    name = "nixpkgs-unstable-2021-05-11";
    url = "https://github.com/nixos/nixpkgs/archive/93123faae0281d2e97d12641a7cfad07c8028aff.tar.gz";
    sha256 = "0kc8rwsfsirr784hh2w143cy2yaqq7in7n5rzjx3j77z7nwsab26";
  }) {
    overlays = [(self: super: {
      # Forked packaging of libhandy with Glade dependency patched out
      # Upstream once Rust 1.50 is on master
      # From 2020-03-09
      libhandy = (with self;
        stdenv.mkDerivation rec {
          pname = "libhandy";
          version = "1.0.3";

          outputs = [ "out" "dev" "devdoc" ];
          outputBin = "dev";

          src = fetchurl {
            url = "mirror://gnome/sources/${pname}/${lib.versions.majorMinor version}/${pname}-${version}.tar.xz";
            sha256 = "sha256-VZuzrMLDYkiJF+ty7SW9wYH0riaslNF3Y0zF00yGf3o=";
          };

          nativeBuildInputs = [
            docbook_xml_dtd_43
            docbook_xsl
            gobject-introspection
            gtk-doc
            libxml2
            meson
            ninja
            pkg-config
            vala
          ];

          buildInputs = [
            gdk-pixbuf
            gtk3
            libxml2
          ];

          checkInputs = [
            dbus
            xvfb_run
            at-spi2-atk
            at-spi2-core
            librsvg
            hicolor-icon-theme
          ];

          mesonFlags = [
            "-Dgtk_doc=true"
            "-Dglade_catalog=disabled"
          ];

          doCheck = true;

          checkPhase = ''
            NO_AT_BRIDGE=1 \
            XDG_DATA_DIRS="$XDG_DATA_DIRS:${hicolor-icon-theme}/share" \
            GDK_PIXBUF_MODULE_FILE="${librsvg.out}/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache" \
            xvfb-run -s '-screen 0 800x600x24' dbus-run-session \
              --config-file=${dbus.daemon}/share/dbus-1/session.conf \
              meson test --print-errorlogs
          '';

          meta = with lib; {
            changelog = "https://gitlab.gnome.org/GNOME/libhandy/-/tags/${version}";
            description = "Building blocks for modern adaptive GNOME apps";
            homepage = "https://gitlab.gnome.org/GNOME/libhandy";
            license = licenses.lgpl21Plus;
            maintainers = teams.gnome.members;
            platforms = platforms.linux;
          };
        }
      );
    })];
  };
in with pkgs;
mkShell rec {
  buildInputs = [
    cargo
    rustc

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
    export LD_LIBRARY_PATH="${lib.makeLibraryPath buildInputs}:''${LD_LIBRARY_PATH}";
    export LIBCLANG_PATH="${llvmPackages.libclang}/lib"
    export GDK_DPI_SCALE=1.3
    export RUST_BACKTRACE=1
    export CARGO_TARGET_DIR=./target
    export RUSTC_WRAPPER=
  '';
}
