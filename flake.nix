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
      devShells = forAllSystems (
        { pkgs }:
        let
          linkedLibs = with pkgs; [
            openssl
            sqlite
          ];
        in
        {
          default = pkgs.mkShell {
            packages =
              (with pkgs; [
                pkg-config
                rustPlatform.bindgenHook
              ])
              ++ linkedLibs;

            env = {
              RUST_LOG = "info,anipler=debug";
            };

            # Sqlx's proc-macros somehow failed to find libsqlite3.so. etc.
            shellHook = ''
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath linkedLibs}:$LD_LIBRARY_PATH"
            '';
          };
        }
      );

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
