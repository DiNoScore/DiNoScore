let
  sources = import ../npins;
  pkgs = import sources.pkgs { };
in
with pkgs;
let
  cargoVendor = rustPlatform.importCargoLock {
    lockFile = ../Cargo.lock;
    outputHashes = {
      "fern-0.6.0" = "sha256-7tyPLPgpNfM4z97vgYlWkNP18BQOoPZqVJbnDhhaR5s=";
      "pipeline-0.6.0" = "sha256-QBwWfynjcMDcfPC3dcWuWmXjPRlKW6Kpq9Q417PTcxI=";
      "xdg-2.5.2" = "sha256-4/lPsoyZseq2eg65J6FPY6uma1p2xrXCiGITQPdZ51Y=";
    };
  };

  # Windows Rust toolchain, assembled from tarballs
  rust-windows =
    let
      version = "1.97.0";
      date = "2026-07-09";
      components = {
        "rustc" = {
          dir = "rustc";
          hash = "sha256-YbVpxYaa4BniNpszXnbxnfroGRteZ073lEaecxPAoLc=";
        };
        "cargo" = {
          dir = "cargo";
          hash = "sha256-2WPaemi+jtDGMYI15znpBI37l0ndTHoLYZ2dEyRjTSc=";
        };
        "rust-std" = {
          dir = "rust-std-x86_64-pc-windows-gnu";
          hash = "sha256-ag65qrfWOTlhgIvFH3JkhWQdo+rFLlr4N/qSHeDrKuw=";
        };
        # Contains some tools but not all, notably no full linker
        "rust-mingw" = {
          dir = "rust-mingw";
          hash = "sha256-vZ1VDpMPCVNzXu+0/hMhb1gPA0734PeQQWIvy1F4uZ0=";
        };
      };
    in
    symlinkJoin {
      name = "rust-windows-${version}";
      paths = lib.mapAttrsToList (
        component:
        { dir, hash }:
        "${fetchzip {
          url = "https://static.rust-lang.org/dist/${date}/${component}-${version}-x86_64-pc-windows-gnu.tar.xz";
          inherit hash;
        }}/${dir}"
      ) components;
    };

  # MinGW-w64 (UCRT)
  # Provides the linker required by Rust, and a gcc required by native C dependencies
  mingw = 
    let
      src = fetchzip {
        url = "https://github.com/brechtsanders/winlibs_mingw/releases/download/14.2.0posix-12.0.0-ucrt-r3/winlibs-x86_64-posix-seh-gcc-14.2.0-mingw-w64ucrt-12.0.0-r3.zip";
        hash = "sha256-BPbRI1czVl+2ehzb0hPNvWS8Ze8ZMapB2ipMVlfhVCc=";
      };
    in
    # Requires some renaming, because some tools expect the simple toolchain name vs the fully prefixed hostarget in the name
    runCommand "mingw-w64-14.2.0" { } ''
      cp -r ${src} $out
      chmod -R u+w $out
      for tool in dlltool as ld ranlib ar nm objcopy objdump; do
        ln $out/bin/$tool.exe $out/bin/x86_64-w64-mingw32-$tool.exe
      done
    '';

  # build.rs calls `blueprint-compiler.exe`, which we don't package for Windows for pain reasons.
  # Instead, we simply manually run the same Linux command beforehand, and put a NOP
  # Windows exe in the place of `blueprint-compiler.exe`
  blueprint-compiler-shim =
    runCommand "blueprint-compiler-shim"
      {
        nativeBuildInputs = [ pkgsCross.mingwW64.buildPackages.gcc ];
      }
      ''
        mkdir -p $out/bin
        echo 'int main(void) { return 0; }' > nop.c
        x86_64-w64-mingw32-gcc nop.c -o $out/bin/blueprint-compiler.exe
      '';

  # GTK4, libadwaita, poppler, librsvg and every transitive runtime dependency, from msys2's UCRT64 repo.
  # Everything is merged into a single tree at `$out/{bin,lib,include,share}/`.
  # .pc files hard-code prefix=/ucrt64 (msys2's native layout); we it to C:/msys2 at build time
  msys2 =
    runCommand "msys2-ucrt64"
      {
        srcs = lib.mapAttrsToList (_: pin: pin { inherit pkgs; }) (
          import ../npins { input = ./msys2.json; }
        );
        nativeBuildInputs = [ zstd gnutar ];
      }
      ''
      mkdir -p $out
      for src in $srcs; do
        tar --zstd -xf $src -C $out --strip-components=1 --skip-old-files ucrt64
      done
      chmod -R u+w $out
      # Globally rewrite some paths to what we hard-code
      sed -i 's|/ucrt64|C:/msys2|g' $out/lib/pkgconfig/*.pc

      # Fix some weird upstream config glitch
      sed -i '/^prefix=${"$"}{prefix}$/d' $out/lib/pkgconfig/*.pc
    '';

  # peldd from gsauthof/pe-util, to extract recursive dll dependencies for creating portable exe files
  # `pkgs.pev` also ships a `peldd`, but it is a different, non-recursive program with different output.
  peldd = stdenv.mkDerivation {
    pname = "pe-util";
    version = "0-unstable-2023-05-18";
    src = fetchFromGitHub {
      owner = "gsauthof";
      repo = "pe-util";
      rev = "dc5dda5cbf89b6d81becaf9d2ddceedad7988346";
      hash = "sha256-ex2fQUn9lSh5Yh4XGMYkHaHkgGiKWa8T6BpXD8rR8qI=";
      fetchSubmodules = true;
    };
    nativeBuildInputs = [ cmake ];
  };

  # Mount points are relative to `C:\`
  # `bin`s of each mount point also go into the Winodws PATH.
  # Order technically matters, but for DLLs files in the same folders will always be preferred over PATH, so it doesn't matter.
  mounts = {
    "rust" = rust-windows;
    "mingw64" = mingw;
    # `C:\msys2` is hard-coded by users (as defined in the msys2 derivation)
    "msys2" = msys2;
    "blueprint-compiler-shim" = blueprint-compiler-shim;
  };
in
stdenv.mkDerivation rec {
  pname = "DiNoScore-windows";
  version = "0.4.0";
  src = (lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../src
      ../res
      ../test
      ../.cargo
      ../Cargo.toml
      ../Cargo.lock
      ../build.rs
    ];
  });
  doCheck = false;
  dontFixup = true;

  # So the expensive inputs can be built and inspected on their own
  passthru = { inherit msys2 peldd rust-windows mingw; };

  nativeBuildInputs = [
    wineWow64Packages.full
    blueprint-compiler
    peldd # transitive PE import resolution, for bundling the DLLs
    glib # glib-compile-schemas; the GVDB output is portable across platforms
  ];

  env = {
    # Tell rustc to use PATH-resolved external tools (mingw64) instead of the incomplete bundled self-contained/ toolchain.
    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = ''-Clink-self-contained=n -L C:\msys2\lib'';
    # Goes into $WINEPATH, which wine prepends to the PATH it reads from the registry
    WINEPATH = lib.concatMapStringsSep ";" (
      dest: "C:\\" + lib.replaceStrings [ "/" ] [ "\\" ] dest + "\\bin"
    ) (lib.attrNames mounts);
    # For -sys crates via the pkg-config Rust crate.
    PKG_CONFIG_PATH = ''C:\msys2\lib\pkgconfig'';
    # msys2 ships the Adwaita icon theme under share/icons
    WINEXDG_DATA_DIRS = ''C:\msys2\share'';
  };

  configurePhase =
    let
      # Like lib.linkFarm but we need to execute it directly in the derivation to keep the folder writable
      mountCommands = lib.concatStrings (
        lib.mapAttrsToList (dest: tree: ''
          mkdir -p "$(dirname "$WINEPREFIX/drive_c/${dest}")"
          ln -s ${tree} "$WINEPREFIX/drive_c/${dest}"
        '') mounts
      );
    in
    ''
      # Set up WINE
      export HOME=$PWD
      export WINEPREFIX=$PWD/wineprefix
      export XDG_CACHE_HOME=$(mktemp -d)
      wineboot -u
      wineserver -w

      ${mountCommands}

      # Vendor cargo dependencies
      # We need to copy things at the top-level instead of symlinking it entirely because it needs to be writable
      cp -rL ${cargoVendor} cargo-vendor-dir
      chmod -R u+w cargo-vendor-dir
      cp ${cargoVendor}/.cargo/config.toml .cargo/config.toml
    '';

  buildPhase = ''
    # build.rs calls `blueprint-compiler.exe`, which we don't package for Windows for pain reasons.
    # Instead, we simply manually run the same Linux command beforehand, and put a NOP Windows exe in the place of `blueprint-compiler.exe`
    chmod -R u+w res
    for blp in res/viewer/*.blp res/editor/*.blp; do
      blueprint-compiler compile --output "''${blp%.blp}.ui" "$blp"
    done

    wine cargo build --release --locked --offline
  '';

  # We want a portable Windows application: All exes and DLLs in the top-level, including transitive dependencies.
  # Bundled resources relative to that, e.g. ./lib
  installPhase = ''
    mkdir -p $out

    cp target/release/viewer.exe target/release/editor.exe target/release/cli.exe $out/

    # Binaries used by gtk which we also need to ship for full functionality
    cp ${msys2}/bin/gdbus.exe \
       ${msys2}/bin/gspawn-win64-helper.exe \
       ${msys2}/bin/gspawn-win64-helper-console.exe \
       $out/

    # GdkPixbuf image loaders for the image formats
    # loaders.cache (see below) contains the list for GdkPixbuf on what exists
    mkdir -p "$out/lib/gdk-pixbuf-2.0/2.10.0/loaders"
    cp ${msys2}/lib/gdk-pixbuf-2.0/2.10.0/loaders/*.dll "$out/lib/gdk-pixbuf-2.0/2.10.0/loaders/"

    # Generates the loaders.cache, which is just a list of paths
    GDK_PIXBUF_MODULEDIR='C:\msys2\lib\gdk-pixbuf-2.0\2.10.0\loaders' \
      wine gdk-pixbuf-query-loaders.exe > "$out/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"

    # Recursively walk all dll dependencies and copy them with `peldd`
    #
    # `-t` walks the import graph
    # `--clear-path` drops peldd's built-in mingw defaults so only our trees are consulted
    # `--ignore-errors` is what skips the OS DLLs: the common ones (kernel32, ...) are on peldd's built-in whitelist, but plenty are not (d3d11, dcomp, HID, the api-ms-win-crt-* stubs) and are reported unresolved on stderr.
    cp -t $out $(
      peldd -t --clear-path --ignore-errors \
        -p ${msys2}/bin -p ${mingw}/bin \
        $out/*.exe $out/lib/gdk-pixbuf-2.0/2.10.0/loaders/*.dll \
    )

    # We can just use Linux' `glib-compile-shchemas` for this
    mkdir -p "$out/share/glib-2.0/schemas"
    glib-compile-schemas --targetdir "$out/share/glib-2.0/schemas" ${msys2}/share/glib-2.0/schemas

    # Icons
    mkdir -p "$out/share/icons"
    cp -r ${msys2}/share/icons/Adwaita "$out/share/icons/"
    cp -r ${msys2}/share/icons/hicolor "$out/share/icons/"

    # Everything copied out of the store came in read-only, and strip rewrites in place
    chmod -R u+w "$out"
    # Strip the copied DLLs & EXEs with a cross toolchain.
    find "$out" -type f \( -iname '*.dll' -o -iname '*.exe' \) -print0 \
      | xargs -0 -r ${pkgsCross.mingwW64.buildPackages.gcc}/bin/x86_64-w64-mingw32-strip
  '';
}
