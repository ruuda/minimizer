{
  description = "Minimizer";

  inputs.nixpkgs.url = "nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }: 
    let
      supportedSystems = [ "x86_64-linux" ];
      # Ridiculous boilerplate required to make flakes somewhat usable.
      forEachSystem = f:
        nixpkgs.lib.zipAttrsWith
          (name: values: builtins.foldl' (x: y: x // y) {} values)
          (map
            (k: builtins.mapAttrs (name: value: { "${k}" = value; }) (f k))
            supportedSystems
          );
    in
      forEachSystem (system:
        let
          name = "minimizer";
          version = "0.1.0";
          pkgs = import nixpkgs { inherit system; };
        in
          rec {
            packages = {
              default = pkgs.rustPlatform.buildRustPackage rec {
                inherit name version;
                src = pkgs.lib.sourceFilesBySuffices ./. [
                  ".rs"
                  "Cargo.lock"
                  "Cargo.toml"
                ];
                cargoLock = {
                  lockFile = ./Cargo.lock;
                  outputHashes = {
                    "brotli-sys-0.3.2" = "sha256-knjFVyjiW03DA5wLw7VxQmaqUJXE6B8Zs0CvoO16QaI=";
                  };
                };
                nativeBuildInputs = [
                  pkgs.pkg-config
                ];
                buildInputs = [
                  pkgs.brotli
                  pkgs.libgit2
                  pkgs.openssl
                ];
              };
            };
          }
      );
}
