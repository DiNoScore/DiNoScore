let
  # Pin nixpkgs
  pkgs = import (builtins.fetchTarball {
    name = "nixpkgs-unstable-2021-05-11";
    url = "https://github.com/nixos/nixpkgs/archive/93123faae0281d2e97d12641a7cfad07c8028aff.tar.gz";
    sha256 = "0kc8rwsfsirr784hh2w143cy2yaqq7in7n5rzjx3j77z7nwsab26";
  }) { };
in with pkgs; let
  appContents = symlinkJoin {
    name = "appimage-test";
    paths = [
      (import ./default.nix)
      (writeTextFile {
        name = "AppRun";
        text = ''
          #!/bin/env sh
          if [ -n "$APPDIR" ]; then
            cd $APPDIR
          fi
          exec ./bin/viewer
        '';
        executable = true;
        destination = "/AppRun";
      })
      (writeTextFile {
        name = "viewer.desktop";
        text = ''
          [Desktop Entry]
          Name=DiNoScore viewer
          Exec=viewer
          Type=Application
          Categories=Utility;
          Icon=nemo
        '';
        destination = "/viewer.desktop";
      })
      (writeTextFile {
        name = "nemo.svg";
        text = "";
        destination = "/nemo.svg";
      })
      (runCommand "source" {} ''
        mkdir -p $out/res
        cp -r ${./res}/* $out/res
      '')

      openssl
#       (python39.withPackages (pypkgs: [
#         pypkgs.pikepdf
#       ]))
      libtensorflow-bin
      bzip2
      glib
      cairo
      atk
      libhandy
      poppler
      gnome3.gtk3
      gdk-pixbuf
      portmidi

      fuse
      glibc
    ];
  };

  appImageGo = buildGoModule rec {
    pname = "appimagekit-go";
    version = "unstable-2021-05-29";
    src = fetchFromGitHub {
      #owner = "probonopd";
      owner = "srevinsaju";
      repo = "go-appimage";
      rev = "5f83e9076083accea284528e7da159d8ce780e01";
      sha256 = "10861agwj7ld9n8vd589zzifznw1rcc5fyn7cvfgmjr9gn2dxiy1";
    };
    doCheck = false;
    
    vendorSha256 = "0hfgx5z5chx5yjbzpcm804n2xy3fx1gzzj7x7z7v6cg9npqv1sqk";

    postInstall = ''
      cp "${builtins.fetchurl "https://github.com/AppImage/AppImageKit/releases/download/continuous/runtime-x86_64"}" $out/bin/runtime-x86_64
    '';
  };

  buildAppImage = appDirContents: runCommandLocal "${appDirContents.name}" {
    nativeBuildInputs = [
      #appimagekit
      #statifier
      appImageGo
      file
      desktop-file-utils
      fuse
      binutils
      
      squashfsTools
      (writeScriptBin "uploadtool" '''')
    ];
  } ''
    unset SOURCE_DATE_EPOCH

    cp -rL ${appDirContents} "AppDir"
    chmod -R +w "AppDir"
    rm -rf "AppDir/sbin" "AppDir/lib64"

    echo "Monkey-patching all binaries"
    find AppDir \
      -exec sh -c "file '{}' | grep -q ELF" \; \
      -exec chmod +w {} \; \
      -exec patchelf --set-rpath "./usr/lib" {} \; \
      -exec sh -c "readelf -e '{}' | grep -q 'Requesting program interpreter'" \; \
      -exec patchelf --set-interpreter "/lib/ld-linux-x86-64.so.2" {} \;

    echo "Building AppImage file"
    mkdir -p "$out"
    mkappimage "AppDir" "$out/${appDirContents.name}"
  '';
in
  buildAppImage appContents
