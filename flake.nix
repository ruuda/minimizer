{
  description = "Minimizer";

  inputs.nixpkgs.url = "nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }: 
    let
      name = "minimizer";
      version = "0.1.0";
      pkgs = (import nixpkgs { system = "x86_64-linux" ; }).pkgsStatic;

      # The default libgit2 package is incompatible with static builds, because
      # its dependency http-parser is built as a dynamic library. But we don't
      # need it anyway, it is vendored by the libgit2 crate.
      libgit2Fixed = (pkgs.libgit2.override {
        staticBuild = true;
        http-parser = null;
      }).overrideAttrs (oldAttrs: {
        cmakeFlags = pkgs.lib.remove "-DUSE_HTTP_PARSER=system" oldAttrs.cmakeFlags;
      });
    in
      {
        packages.x86_64-linux.default = pkgs.rustPlatform.buildRustPackage rec {
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
          buildInputs = with pkgs; [
            brotli
            openssl
            libssh2
            pcre
            zlib
            libgit2Fixed
          ];
          RUSTFLAGS = "-lbrotlidec -lbrotlicommon -lbrotlienc -lm -lssl -lc";
        };
      };
}
