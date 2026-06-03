{
  description = "patent — prior-art search for your code ideas";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        src = pkgs.lib.cleanSource ./.;

        cargoToml = pkgs.lib.importTOML ./Cargo.toml;

        # `fastembed` pulls in `ort` / `ort-sys`, whose build script downloads
        # prebuilt ONNX Runtime binaries from the network — which fails inside
        # the Nix sandbox (no DNS / no network).
        #
        # Instead we fetch the *exact* static library that `ort` 2.0.0-rc.12
        # would download (ONNX Runtime 1.24.2, C API version 24 — the version
        # required by `fastembed`'s `ort/api-24` feature) as a fixed-output
        # derivation, then point `ORT_LIB_LOCATION` at it. `ort-sys`'s build
        # script then links that library directly and never touches the network.
        ortVersion = "1.24.2";

        ortDist = {
          "x86_64-linux" = {
            target = "x86_64-unknown-linux-gnu";
            sha256 = "acc1cba79c337594ead1d88ca72516147aa60054c84217b53399a31caa5ba671";
          };
          "aarch64-linux" = {
            target = "aarch64-unknown-linux-gnu";
            sha256 = "7e4f5fec4494cbf578c4e28082b0229c42f735523f584259028dde96acf3b092";
          };
          "aarch64-darwin" = {
            target = "aarch64-apple-darwin";
            sha256 = "612739f75438dc0a075461e1fb454226b4a1eb175e60a7271ba966bbbb972cd4";
          };
        }.${system} or (throw "patent: no prebuilt ONNX Runtime for system '${system}'");

        onnxruntimeStatic = pkgs.stdenvNoCC.mkDerivation {
          pname = "onnxruntime-ort-static";
          version = ortVersion;

          src = pkgs.fetchurl {
            url = "https://cdn.pyke.io/0/pyke:ort-rs/ms@${ortVersion}/${ortDist.target}.tar.lzma2";
            inherit (ortDist) sha256;
          };

          nativeBuildInputs = [ pkgs.xz ];

          # pyke ships a raw LZMA2 stream (not an `.xz` container) wrapping a tar
          # archive, so it has to be decoded with an explicit filter chain.
          unpackPhase = ''
            runHook preUnpack
            xz --format=raw --lzma2=dict=64MiB -dc "$src" | tar xf -
            runHook postUnpack
          '';

          dontConfigure = true;
          dontBuild = true;

          installPhase = ''
            runHook preInstall
            mkdir -p "$out/lib"
            cp libonnxruntime.a "$out/lib/"
            runHook postInstall
          '';
        };

        patent = rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          inherit src;
          cargoLock.lockFile = "${src}/Cargo.lock";

          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ openssl ];

          env.OPENSSL_NO_VENDOR = 1;
          env.ORT_LIB_LOCATION = "${onnxruntimeStatic}/lib";

          # rank() integration tests download the embedding model (network).
          doCheck = false;
        };
      in
      {
        packages.default = patent;
        packages.patent = patent;

        apps.default = flake-utils.lib.mkApp { drv = patent; };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ patent ];
          packages = with pkgs; [
            rustToolchain
            pkg-config
            openssl
          ];
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          OPENSSL_NO_VENDOR = 1;
          ORT_LIB_LOCATION = "${onnxruntimeStatic}/lib";
        };

        formatter = pkgs.nixpkgs;
      });
}
