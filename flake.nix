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
    let
      perSystem = flake-utils.lib.eachDefaultSystem (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          toolchain = fenix.packages.${system}.stable.toolchain;
        in
        {
          packages.default = pkgs.callPackage ./dist/nix/default.nix { };

          devShells.default = pkgs.mkShell {
            name = "luks-controller-unlock-dev";

            nativeBuildInputs = with pkgs; [
              toolchain
              pkg-config
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

            env.RUST_BACKTRACE = "1";
          };

          formatter = pkgs.nixpkgs-fmt;
        }
      );
    in
    perSystem // {
      # System-independent NixOS module. Consume from a downstream
      # flake as `inputs.luks-controller-unlock.nixosModules.default`.
      nixosModules.default = import ./dist/nix/module.nix;
    };
}
