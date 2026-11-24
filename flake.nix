# Copyright (C) 2026 EncidXVII
# SPDX-License-Identifier: GPL-3.0-or-later

# WristXR for Nix and NixOS, from the published Linux release (the same files as the tarball on GitHub Releases).
#   nix run github:EncidXVII/WristXR
# WristXR registers its own path with SteamVR so SteamVR can start it, and a Nix store path changes with every update.
# So the `wristxr` command keeps a per-user copy in ~/.local/share/wristxr (the folder install.sh uses), refreshes it
# when the package changes, and runs that copy. Uninstall: `wristxr --uninstall`, then delete that folder.
{
  description = "WristXR: your PC desktop, a wrist watch and notifications in SteamVR";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      version = "0.9.0";
      # The release file for each system and its SHA-256 (the .sha256 file beside it, in Nix's SRI form).
      assets = {
        x86_64-linux = {
          arch = "x64";
          hash = "sha256-nVFuqp7O0DyD5J74wNJhvBHUCUamIGUU0bU33eygzzw=";
        };
        aarch64-linux = {
          arch = "arm64";
          hash = "sha256-dvTqNp7RQYL0itUOmDkK3x67Ecothig7DKQzcGKDEAM=";
        };
      };
      forEach = f: nixpkgs.lib.genAttrs (builtins.attrNames assets) (system: f nixpkgs.legacyPackages.${system} assets.${system});
    in
    {
      packages = forEach (
        pkgs: asset:
        let
          meta = {
            description = "Your PC desktop, a wrist watch and notifications in SteamVR";
            homepage = "https://everyvr.net/wristxr";
            license = pkgs.lib.licenses.gpl3Plus;
            sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
            platforms = builtins.attrNames assets;
            mainProgram = "wristxr";
          };
          # The release as published, with its libraries pointed at the Nix store. Vulkan, PipeWire (screen capture
          # on Wayland), libxkbcommon (keyboard labels on X11) and the OpenXR loader (Monado, WiVRn) are loaded at
          # run time, so they go on the library path too.
          app = pkgs.stdenv.mkDerivation {
            pname = "wristxr-app";
            inherit version meta;
            src = pkgs.fetchurl {
              url = "https://github.com/EncidXVII/WristXR/releases/download/v${version}/WristXR-${version}-linux-${asset.arch}.tar.gz";
              inherit (asset) hash;
            };
            nativeBuildInputs = [ pkgs.autoPatchelfHook ];
            buildInputs = [
              pkgs.stdenv.cc.cc.lib
              pkgs.fontconfig
            ];
            runtimeDependencies = [
              pkgs.vulkan-loader
              pkgs.pipewire
              pkgs.libxkbcommon
              pkgs.openxr-loader
            ];
            dontConfigure = true;
            dontBuild = true;
            installPhase = ''
              runHook preInstall
              mkdir -p $out/share/wristxr
              cp -r wristxr README.md LICENSE NOTICE THIRD_PARTY_NOTICES.md uninstall.sh $out/share/wristxr/
              runHook postInstall
            '';
          };
          wristxr = pkgs.writeShellApplication {
            name = "wristxr";
            runtimeInputs = [ pkgs.coreutils ];
            inherit meta;
            text = ''
              dest="''${XDG_DATA_HOME:-$HOME/.local/share}/wristxr"
              if [ "$(cat "$dest/.nix-package" 2>/dev/null || true)" != "${app}" ]; then
                mkdir -p "$dest"
                cp -R --remove-destination "${app}/share/wristxr/." "$dest/"
                chmod -R u+w "$dest"
                echo "${app}" > "$dest/.nix-package"
              fi
              # The copy's libraries are in the Nix store: a GC root keeps them while the copy is there, or a
              # garbage collection after `nix run` would break the copy.
              if command -v nix-store >/dev/null; then
                nix-store --add-root "$dest/.nix-gcroot" --realise "${app}" >/dev/null 2>&1 || true
              fi
              exec "$dest/wristxr" "$@"
            '';
          };
        in
        {
          inherit app wristxr;
          default = wristxr;
        }
      );
    };
}
