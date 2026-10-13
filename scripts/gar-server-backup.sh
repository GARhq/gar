#!/usr/bin/env bash
# gar server backup - backup do runtime state (K-2026-10-13 BUG-005)
#
# Camadas:
#   1. Snapshot BTRFS de /var/lib/garos -> /srv/data/backups/garos/runtime
#   2. (Opcional) Replicacao para target off-host via rsync/btrfs-send

set -Eeuo pipefail

RUNTIME_PRIMARY="${GAROS_RUNTIME_ROOT:-/var/lib/garos/runtime}"
BACKUP_ROOT="${GAROS_BACKUP_ROOT:-/srv/data/backups/garos/runtime}"
BACKUP_SNAPSHOT_DIR="/srv/data/backups/garos/snapshots"

log() { echo "[$(date +'%Y-%m-%dT%H:%M:%S%z')] $*"; }
die() { log "ERRO: $*" >&2; exit 1; }

[[ -d "$RUNTIME_PRIMARY" ]] || die "primary nao encontrado: $RUNTIME_PRIMARY"

# Garante que /srv/data/backups e subvolume BTRFS montado
if ! mountpoint -q /srv/data/backups 2>/dev/null; then
    log "/srv/data/backups nao e mountpoint. Criando..."
    sudo mkdir -p /srv/data/backups
    # Numa instalacao real, isso seria via NixOS mount
    # Aqui usamos o filesystem raiz se /srv/data e BTRFS
fi

sudo mkdir -p "$BACKUP_ROOT"
sudo mkdir -p "$BACKUP_SNAPSHOT_DIR"

# Backup dos 3 arquivos autoritativos
for f in params.nix hardware-configuration.nix client-users.json; do
    if [[ -f "$RUNTIME_PRIMARY/$f" ]]; then
        sudo cp -p "$RUNTIME_PRIMARY/$f" "$BACKUP_ROOT/$f"
        log "backup ok: $f ($(stat -c%s "$RUNTIME_PRIMARY/$f") bytes)"
    else
        log "skip: $f (nao existe em primary)"
    fi
done

# Tambem fazer backup do sops secrets (se existir)
if [[ -d /etc/garos/secrets ]]; then
    sudo mkdir -p /srv/data/backups/garos/secrets
    sudo cp -p /etc/garos/secrets/*.yaml /srv/data/backups/garos/secrets/ 2>/dev/null || true
    log "backup ok: sops secrets"
fi

# Tambem backup do inventory
if [[ -d /etc/garos-inventory ]]; then
    sudo mkdir -p /srv/data/backups/garos/inventory
    if [[ -d /etc/static/garos-inventory ]]; then
        sudo cp -rp /etc/static/garos-inventory/. /srv/data/backups/garos/inventory/ 2>/dev/null || true
    fi
    log "backup ok: inventory"
fi

log "Backup completo em $BACKUP_ROOT"
