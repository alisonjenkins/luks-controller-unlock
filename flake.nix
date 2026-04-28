{
  description = "luks-controller-unlock — unlock LUKS2 volumes with a game controller";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, flake-utils, fenix, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        toolchain = fenix.packages.${system}.stable.toolchain;
      in
      {
        devShells.default = pkgs.mkShell {
          name = "luks-controller-unlock-dev";

          nativeBuildInputs = with pkgs; [
            toolchain
            pkg-config
            gcc
            binutils
            cmake
          ];

          buildInputs = with pkgs; [
            libdrm
            systemdMinimal
            cryptsetup
          ];

          packages = with pkgs; [
            cargo-edit
            cargo-watch
            cargo-nextest
            rust-analyzer
          ];

          env = {
            RUST_BACKTRACE = "1";
            PKG_CONFIG_PATH = "${pkgs.libdrm.dev}/lib/pkgconfig:${pkgs.systemdMinimal.dev}/lib/pkgconfig";
            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
          };

          shellHook = ''
            echo "luks-controller-unlock dev shell"
            echo "  rustc: $(rustc --version)"
            echo "  cargo: $(cargo --version)"
          '';
        };

        formatter = pkgs.nixpkgs-fmt;
      }
    );
}
