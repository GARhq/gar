# GAR CLI — Nix flake
#
# Exposes the pre-built `gar` binary as a flake output so the parent
# RAGOS monorepo can consume it as a flake input:
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
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        rustToolchain = pkgs.rustc;
        cargo = pkgs.cargo;
      in {
        packages.default = pkgs.callPackage ./default.nix {
          rustc = rustToolchain;
          cargo = cargo;
        };

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
        };
      }) // {
        # Cross-platform override map (so non-default systems can still resolve).
        overlays.default = final: prev: {
          gar = prev.callPackage ./default.nix { };
        };
      };
}
