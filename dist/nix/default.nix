{ lib, rustPlatform, pkg-config, libdrm, ... }:

rustPlatform.buildRustPackage {
  pname = "luks-controller-unlock";
  version = "0.1.0";

  src = lib.cleanSource ../..;

  cargoLock.lockFile = ../../Cargo.lock;

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ libdrm ];

  meta = with lib; {
    description = "Unlock LUKS2 volumes using a game controller";
    homepage = "https://github.com/ali/luks-controller-unlock";
    license = with licenses; [ mit asl20 ];
    platforms = platforms.linux;
    mainProgram = "luks-controller-unlock";
  };
}
