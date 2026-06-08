{
  description = "patent — prior-art search for your code ideas";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable.latest.default;

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        src = pkgs.lib.cleanSource ./.;

        cargoToml = pkgs.lib.importTOML ./Cargo.toml;

        patent = rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          inherit src;
          cargoLock.lockFile = "${src}/Cargo.lock";

          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [
            openssl
            onnxruntime
          ];

          env.OPENSSL_NO_VENDOR = 1;
          env.ORT_PREFER_DYNAMIC_LINK = 1;
          env.ORT_LIB_LOCATION = "${pkgs.onnxruntime}/lib";

          # rank() integration tests download the embedding model (network).
          doCheck = false;
        };

        dockerImage = pkgs.dockerTools.buildImage {
          name = patent.pname;
          tag = patent.version;

          copyToRoot = pkgs.buildEnv {
            name = "image-root";
            paths = [
              patent
              pkgs.cacert
            ];
            pathsToLink = [
              "/bin"
              "/etc"
            ];
          };

          config = {
            Entrypoint = [ "/bin/patent" ];
            Env = [ "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt" ];
          };
        };
      in
      {
        packages.default = patent;
        packages.patent = patent;
        packages.dockerImage = dockerImage;

        apps.default = flake-utils.lib.mkApp { drv = patent; };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ patent ];
          packages = with pkgs; [
            rustToolchain
            pkg-config
            openssl
          ];
          OPENSSL_NO_VENDOR = 1;
          ORT_PREFER_DYNAMIC_LINK = 1;
          ORT_LIB_LOCATION = "${pkgs.onnxruntime}/lib";
        };

        formatter = pkgs.nixfmt;
      }
    );
}
