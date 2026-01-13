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
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              openssl
              pkg-config
              rustPlatform.bindgenHook
              sqlite
            ];

            env = {
              RUST_LOG = "trace";
            };
          };
        }
      );
    };
}
