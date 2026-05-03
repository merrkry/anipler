{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    crane.url = "github:ipetkov/crane";
  };

  outputs =
    { self, ... }@inputs:
    let
      supportedSystems = [
        "x86_64-linux"
      ];

      forAllSystems =
        mkFlakeOutput:
        inputs.nixpkgs.lib.genAttrs supportedSystems (
          system:
          mkFlakeOutput {
            inherit system;
            pkgs = import inputs.nixpkgs { inherit system; };
          }
        );
    in
    {
      nixosModules.default =
        { lib, pkgs, ... }:
        {
          imports = [ ./nix/nixos.nix ];

          services.anipler.package =
            lib.mkDefault
              self.packages.${pkgs.stdenv.hostPlatform.system}.anipler-daemon;
        };

      packages = forAllSystems (
        { pkgs, ... }:
        let
          craneLib = inputs.crane.mkLib pkgs;
          commonArgs = {
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          buildBin =
            name:
            craneLib.buildPackage (
              commonArgs
              // {
                inherit cargoArtifacts;
                pname = name;
                cargoExtraArgs = "--bin ${name}";

                meta = {
                  mainProgram = name;
                };
              }
            );
        in
        rec {
          default = anipler-daemon;
          anipler-daemon = buildBin "anipler-daemon";
          anipler-puller = buildBin "anipler-puller";
        }
      );
    };
}
