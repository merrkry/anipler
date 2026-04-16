{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

  outputs =
    { self, ... }@inputs:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems =
        mkFlakeOutput:
        inputs.nixpkgs.lib.genAttrs supportedSystems (
          system:
          mkFlakeOutput {
            pkgs = import inputs.nixpkgs { inherit system; };
          }
        );

    in
    {
      nixosModules.default = import ./nix/nixos.nix;

      packages = forAllSystems (
        { pkgs }:
        rec {
          default = anipler-nightly;
          anipler-nightly = pkgs.callPackage ./nix/package.nix { };
        }
      );
    };
}
