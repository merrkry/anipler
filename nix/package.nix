{
  lib,
  rustPlatform,
  openssl,
  pkg-config,
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
  # TODO: migrate to crane/naersk.
  cargoDeps = rustPlatform.fetchCargoVendor {
    inherit src;
    hash = "sha256-pwnRHwwSXTszSlPrmYXid7QL1+tP9V2WKJmAno69CX8=";
  };

  nativeBuildInputs = [
    pkg-config
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    openssl
    sqlite
  ];

  meta = {
    mainProgram = "anipler";
  };
}
