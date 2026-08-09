# gar — GAR CLI

Unified manager for **GAROS diskless clients** and **NixOS server**.

Replaces two legacy Bash CLIs:

| Legacy | Replaced by |
|---|---|
| `ragc` (image management) | `gar image <verb>` |
| `ragos` (server operations) | `gar server`, `gar user`, `gar group`, `gar client`, `gar branding` |

## Installation

```bash
cargo build --release
# Binary at: target/release/gar
```

## Usage

```
gar <verb> <noun> [options]

Verbs:
  image     Manage client diskless images
  server    Manage NixOS server
  user      Manage users
  group     Manage groups
  client    Client diagnostics
  branding  Branding diagnostics

Global options:
  --json      Output JSON
  -v          Verbose
  --no-color  No colors
```

## Examples

```bash
# Build a new client image and atomically promote it
gar image build --target desktop-generic --channel generic

# Rollback to previous generation
gar image rollback

# List all generations
gar image list

# Server operations
gar server sync
gar server switch
gar server status

# User management
gar user add alice --quota 20G --password secret
gar user list
gar user doctor alice
```

## Architecture

```
gar/
├── src/
│   ├── main.rs              Entry + tracing
│   ├── cli.rs               clap definitions
│   ├── error.rs             GarError + Result<T>
│   ├── config.rs            env vars + paths
│   ├── output.rs            JSON/table/colors
│   ├── commands/
│   │   ├── image/           era ragc (build, rollback, list, status, gc, doctor)
│   │   ├── server.rs        era ragos top-level
│   │   ├── user.rs          era ragos user
│   │   ├── group.rs         era ragos group
│   │   ├── client.rs        era ragos client
│   │   └── branding.rs      era ragos branding
│   └── services/
│       ├── shell.rs         Process spawning
│       ├── atomic_file.rs   Atomic writes (temp + rename)
│       ├── git.rs           Git operations
│       ├── nix.rs           Nix flake operations
│       ├── nixos_rebuild.rs nixos-rebuild wrappers
│       ├── btrfs.rs         BTRFS subvolume/quota/snapshot
│       ├── user_system.rs   useradd/usermod/userdel
│       └── group_system.rs  groupadd/gpasswd
└── tests/
```

## Compatibility

For 6 months, legacy `ragc` and `ragos` will remain as shims that
delegate to `gar`:

```bash
# Old (still works)
ragc switch --target desktop-generic
ragos user add alice --quota 20G

# New (preferred)
gar image build --target desktop-generic
gar user add alice --quota 20G
```

## Status

| Fase | Status |
|---|---|
| Fase 1: Esqueleto + `gar image build` | ✅ **Done** (round 17) |
| Fase 2: Image rollback/list/status/gc/doctor | 🔜 Next |
| Fase 3: Server subcommand | 📋 Planned |
| Fase 4: User subcommand | 📋 Planned |
| Fase 5: Group subcommand | 📋 Planned |
| Fase 6: Client + branding | 📋 Planned |
| Fase 7: Path migration | 📋 Planned |
| Fase 8: Shims `ragc`/`ragos` | 📋 Planned |

See [[../../garos-think-vault/07-Kanban/K-003-unificar-cli-ragc-ragos-em-gar]] for details.

## License

MIT