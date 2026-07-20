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
      # Explicit system list: nixpkgs 26.11 dropped x86_64-darwin, so
      # enumerating flakeExposed (which still lists it) throws when the
      # merged output set forces every system's legacyPackages. Only the
      # systems this project actually targets are listed.
      perSystem = flake-utils.lib.eachSystem [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ] (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          fenixPkgs = fenix.packages.${system};
          isDarwin = pkgs.stdenv.hostPlatform.isDarwin;

          linuxNativeShell = pkgs.mkShell {
            name = "luks-controller-unlock-dev";

            nativeBuildInputs = [
              fenixPkgs.stable.toolchain
              pkgs.pkg-config
              pkgs.cmake
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

          # On darwin, sidestep nixpkgs' currently-broken cross-perl by
          # using zig's bundled cross linker and pulling libdrm as a
          # native aarch64-linux build (delegated to linux-builder).
          darwinCrossShell =
            let
              crossTarget = "aarch64-unknown-linux-gnu";
              crossEnvName = pkgs.lib.toUpper
                (pkgs.lib.replaceStrings [ "-" ] [ "_" ] crossTarget);
              crossToolchain = fenixPkgs.combine [
                fenixPkgs.stable.toolchain
                fenixPkgs.targets.${crossTarget}.stable.rust-std
              ];
              linuxLibdrm = nixpkgs.legacyPackages.aarch64-linux.libdrm;
              zigLinker = pkgs.writeShellScript "zig-cc-aarch64-linux" ''
                exec ${pkgs.zig}/bin/zig cc -target aarch64-linux-gnu "$@"
              '';
            in
            pkgs.mkShell {
              name = "luks-controller-unlock-dev-cross";

              nativeBuildInputs = [
                crossToolchain
                pkgs.pkg-config
                pkgs.zig
              ];

              packages = (with pkgs; [
                cargo-edit
                cargo-watch
                cargo-nextest
              ]) ++ [ fenixPkgs.rust-analyzer ];

              env = {
                RUST_BACKTRACE = "1";
                CARGO_BUILD_TARGET = crossTarget;
                "CARGO_TARGET_${crossEnvName}_LINKER" = "${zigLinker}";
                PKG_CONFIG_PATH = "${linuxLibdrm.dev}/lib/pkgconfig";
                PKG_CONFIG_SYSROOT_DIR = "${linuxLibdrm.dev}";
                PKG_CONFIG_ALLOW_CROSS = "1";
              };
            };
        in
        {
          devShells.default = if isDarwin then darwinCrossShell else linuxNativeShell;

          formatter = pkgs.nixpkgs-fmt;
        } // pkgs.lib.optionalAttrs (!isDarwin) {
          packages.default = pkgs.callPackage ./dist/nix/default.nix { };
        }
      );
    in
    perSystem // {
      # System-independent NixOS module. Consume from a downstream
      # flake as `inputs.luks-controller-unlock.nixosModules.default`.
      nixosModules.default = import ./dist/nix/module.nix;
    };
}
