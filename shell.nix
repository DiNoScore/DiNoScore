{pkgs ? import <nixpkgs> { }}:
with pkgs;
let
  libhandy1 = stdenv.mkDerivation rec {
  pname = "libhandy1";
  version = "0.90.0";

  outputs = [ "out" "dev" "devdoc" "glade" ];
  outputBin = "dev";

  src = fetchFromGitLab {
    domain = "gitlab.gnome.org";
    owner = "GNOME";
    repo = "libhandy";
    rev = "${version}";
    sha256 = "1bl2gsx364ahz48j5srzn6c5znlv1v1syk4svqlfzqi1ax4lsrqq";
  };

  nativeBuildInputs = [
    meson ninja pkgconfig gobject-introspection vala libxml2
    gtk-doc docbook_xsl docbook_xml_dtd_43
  ];
  buildInputs = [ gnome3.gnome-desktop gtk3 glade libxml2 ];
  checkInputs = [ dbus xvfb_run hicolor-icon-theme ];

  mesonFlags = [
    "-Dgtk_doc=true"
    "-Dglade_catalog=enabled"
    "-Dintrospection=enabled"
  ];

  PKG_CONFIG_GLADEUI_2_0_MODULEDIR = "${placeholder "glade"}/lib/glade/modules";
  PKG_CONFIG_GLADEUI_2_0_CATALOGDIR = "${placeholder "glade"}/share/glade/catalogs";

  doCheck = false;

  checkPhase = ''
    NO_AT_BRIDGE=1 \
    XDG_DATA_DIRS="$XDG_DATA_DIRS:${hicolor-icon-theme}/share" \
    xvfb-run -s '-screen 0 800x600x24' dbus-run-session \
      --config-file=${dbus.daemon}/share/dbus-1/session.conf \
      meson test --print-errorlogs
  '';

  meta = with stdenv.lib; {
    description = "A library full of GTK widgets for mobile phones";
    homepage = "https://gitlab.gnome.org/GNOME/libhandy";
    license = licenses.lgpl21Plus;
    maintainers = with maintainers; [ jtojnar ];
    platforms = platforms.linux;
  };
};

in
 mkShell rec {
  buildInputs = [
   cargo
   poppler
   poppler_data
   pkg-config
   gnome3.gtk3
   gdk-pixbuf
   atk
   libhandy1
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
  '';
 }
