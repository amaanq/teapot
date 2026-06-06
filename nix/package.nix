{
  lib,
  rustPlatform,
  pkg-config,
}:
rustPlatform.buildRustPackage {
  pname = "teapot";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../public
      ../config
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ pkg-config ];

  doCheck = false;
  stripAllList = [ "bin" ];

  postInstall = ''
    mkdir -p $out/share/teapot
    cp -r public $out/share/teapot/
    cp -r config $out/share/teapot/
  '';

  meta = {
    description = "A privacy-focused Twitter/X frontend written in Rust";
    homepage = "https://github.com/amaanq/teapot";
    license = lib.licenses.agpl3Only;
    mainProgram = "teapot";
  };
}
