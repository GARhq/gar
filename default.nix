# Default package: build `gar` from source via cargo.
#
# Uses `rustPlatform.buildRustPackage` so we don't have to hand-roll the
# dependency closure (Cargo.lock is the source of truth).
{ rustPlatform
, lib
, openssl
, pkg-config
, nix
, btrfs-progs
, findutils
, coreutils
, gawk
, gnugrep
, gnused
, jq
, util-linux
, shadow
, nfs-utils
, iproute2
, curl
, systemd
, makeWrapper
}:

rustPlatform.buildRustPackage {
  pname = "gar";
  version = "0.1.0";

  src = ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [ pkg-config makeWrapper ];

  buildInputs = [ openssl ];

  checkFlags = [ "--" "--test-threads=1" ];

  meta = with lib; {
    description = "GAR CLI — Unified manager for GAROS diskless clients and NixOS server";
    license = licenses.mit;
    platforms = platforms.unix;
    mainProgram = "gar";
  };

  # Runtime PATH so `gar` can find the binaries it shells out to
  # (groupadd/useradd/btrfs/qgroup/xfs_quota/zfs/etc).
  postInstall = ''
    wrapProgram $out/bin/gar \
      --prefix PATH : ${lib.makeBinPath [
        nix btrfs-progs findutils coreutils gawk gnugrep gnused
        jq util-linux shadow nfs-utils iproute2 curl systemd
      ]}
  '';
}
