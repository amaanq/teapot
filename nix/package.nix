{
  lib,
  rustPlatform,
  stdenv,
  pkg-config,
  makeWrapper,
  ffmpeg-headless,
  clang,
  wild ? null,
}:
let
  hasWild =
    stdenv.hostPlatform.isLinux && (stdenv.hostPlatform.isx86_64 || stdenv.hostPlatform.isAarch64);
in
rustPlatform.buildRustPackage {
  pname = "teapot";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../LICENSE
      ../src
      ../public
      ../config/teapot.example.toml
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    makeWrapper
  ]
  ++ lib.optionals hasWild [
    wild
    clang
  ];

  env = lib.optionalAttrs hasWild {
    RUSTFLAGS = "-Clinker=${clang}/bin/clang -Clink-arg=--ld-path=wild";
  };

  doCheck = true;
  stripAllList = [ "bin" ];

  postInstall = ''
    mkdir -p $out/share/teapot
    cp -r public $out/share/teapot/
    mkdir -p $out/share/teapot/config
    cp config/teapot.example.toml $out/share/teapot/config/
    wrapProgram $out/bin/teapot \
      --prefix PATH : ${lib.makeBinPath [ ffmpeg-headless ]}
  '';

  meta = {
    description = "A privacy-focused Twitter/X frontend written in Rust";
    homepage = "https://github.com/amaanq/teapot";
    license = lib.licenses.agpl3Only;
    mainProgram = "teapot";
  };
}
