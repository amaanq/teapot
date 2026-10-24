{
  lib,
  rustPlatform,
}:
let
  topcoat = lib.findFirst (pkg: pkg.name == "topcoat") (throw "Cargo.lock has no topcoat") (
    (lib.importTOML ../Cargo.lock).package
  );
  src = builtins.fetchGit {
    url = "https://github.com/amaanq/topcoat";
    rev = lib.last (lib.splitString "#" topcoat.source);
  };
in
rustPlatform.buildRustPackage {
  pname = "topcoat-cli";
  inherit (topcoat) version;
  inherit src;

  cargoLock.lockFile = "${src}/Cargo.lock";
  cargoBuildFlags = [
    "--package"
    "topcoat-cli"
  ];
  doCheck = false;

  meta.mainProgram = "topcoat";
}
