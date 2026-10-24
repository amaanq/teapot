{
  lib,
  rustPlatform,
  stdenv,
  pkg-config,
  makeWrapper,
  ffmpeg-headless,
  buildPackages,
  topcoat-cli,
}:
let
  inherit (buildPackages) clang wild;
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

  cargoLock = {
    lockFile = ../Cargo.lock;
    allowBuiltinFetchGit = true;
  };

  nativeBuildInputs = [
    pkg-config
    makeWrapper
    topcoat-cli
  ]
  ++ lib.optionals hasWild [
    wild
    clang
  ];

  env = lib.optionalAttrs hasWild {
    RUSTFLAGS = "-Clinker=${clang}/bin/${clang.targetPrefix}clang -Clink-arg=--ld-path=wild";
  };

  postBuild = ''
    CARGO_BUILD_TARGET=${stdenv.hostPlatform.rust.rustcTarget} \
      topcoat asset bundle --release --out topcoat-assets
  '';

  doCheck = true;
  stripAllList = [ "bin" ];

  postInstall = ''
    mkdir -p $out/share/teapot
    cp -r public $out/share/teapot/
    mkdir -p $out/share/teapot/config
    cp config/teapot.example.toml $out/share/teapot/config/
    cp -r topcoat-assets $out/share/teapot/assets
    wrapProgram $out/bin/teapot \
      --prefix PATH : ${lib.makeBinPath [ ffmpeg-headless ]} \
      --set-default TEAPOT_ASSETS_DIR $out/share/teapot/assets
  '';

  meta = {
    description = "A privacy-focused Twitter/X frontend written in Rust";
    homepage = "https://github.com/amaanq/teapot";
    license = lib.licenses.agpl3Only;
    mainProgram = "teapot";
  };
}
