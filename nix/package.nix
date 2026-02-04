{
  lib,
  rustPlatform,
  openssl,
  pkg-config,
  rsync,
  sqlite,
}:
rustPlatform.buildRustPackage rec {
  pname = "anipler";
  version = "nightly";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../src
      ../Cargo.lock
      ../Cargo.toml
    ];
  };

  # Alternatively we can use `importCargoLock` without the need to specify hash,
  # but it fetches EVERY dependency as separate FOD, which feels scary.
  cargoDeps = rustPlatform.fetchCargoVendor {
    inherit src;
    hash = "sha256-vsDlbr/kKsDzQKnUcr69N76kiolMguWh+JO9GtAx9fw=";
  };

  nativeBuildInputs = [
    pkg-config
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    openssl
    rsync
    sqlite
  ];

  meta = {
    mainProgram = "anipler";
  };
}
