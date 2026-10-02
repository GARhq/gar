# GAR CLI — Nix flake
#
# Exposes the pre-built `gar` binary as a flake output so the parent
# GAROS monorepo can consume it as a flake input:
#
#   inputs.gar-cli.url = "github:GARhq/gar";
#   inputs.gar-cli.flake = true;
#
#   environment.systemPackages = [ inputs.gar-cli.packages.${system}.default ];
#
# Build path: `nix build .#gar` (or `nix build github:GARhq/gar`).
# Dev shell: `nix develop` (provides cargo/rustc/cargo-edit/rustfmt/clippy).
{
  description = "GAR CLI — Unified manager for GAROS diskless clients and NixOS server";

  inputs = {
    # K-008: migração NixOS 26.05 LTS
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        rustToolchain = pkgs.rustc;
        cargo = pkgs.cargo;
      in {
        packages.default = pkgs.callPackage ./default.nix { };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/gar";
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = with pkgs; [
            rustc
            cargo
            rustfmt
            clippy
            pkg-config
          ];
          # K-dev: auto-update flake lock quando flake.nix for modificado.
          shellHook = ''
            _flake_nix="$PWD/flake.nix"
            _flake_lock="$PWD/flake.lock"
            if [[ -f "$_flake_nix" && -f "$_flake_lock" ]] && [[ "$_flake_nix" -nt "$_flake_lock" ]]; then
              printf '\033[1;33m[rebuild]\033[0m flake.nix modificado -> running nix flake update\n' >&2
              if nix flake update 2>&1 | sed 's/^/  /' >&2; then
                printf '\033[1;32m[rebuild]\033[0m flake.lock atualizado\n' >&2
              else
                printf '\033[1;31m[rebuild]\033[0m nix flake update FALHOU\n' >&2
              fi
            fi
          '';
        };
      }) // {
        # Cross-platform override map (so non-default systems can still resolve).
        overlays.default = final: prev: {
          gar = prev.callPackage ./default.nix { };
        };
      };
}
