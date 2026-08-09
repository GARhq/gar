# ragos → gar migration

Mapping from the legacy Nix-based `ragos` + `ragc` CLIs to the unified
Rust-based `gar` CLI. Updated as each Phase is implemented and merged.

## Status legend

| Symbol | Meaning |
|---|---|
| ✅ | implemented + tested in `gar` |
| ⏳ | pending migration |
| ❌ | intentionally not migrated (infra-only, lives in Nix modules) |

## Command mapping

| ragos / ragc command | gar command | Status | Phase |
|---|---|---|---|
| `ragos group add` | `gar group add` | ✅ | Phase 1 |
| `ragos group list` | `gar group list` | ✅ | Phase 1 |
| `ragos group delete` | `gar group delete` | ✅ | Phase 1 |
| `ragos group chmod` | `gar group chmod` | ✅ | Phase 1 |
| `ragos group members` | `gar group members` | ✅ | Phase 1 |
| `ragos group permissions` | `gar group permissions` | ✅ | Phase 1 |
| `ragos group ensure-defaults` | `gar group ensure-defaults` | ✅ | Phase 1 |
| `ragos branding doctor` | `gar branding doctor` | ✅ | Phase 2.1 |
| `ragos client session-doctor` | `gar client session-doctor` | ✅ | Phase 2.2 |
| `ragos user add` | `gar user add` | ✅ | pre-existing |
| `ragos user resize` | `gar user resize` | ✅ | pre-existing |
| `ragos user list` | `gar user list` | ✅ | pre-existing |
| `ragos user delete` | `gar user delete` | ✅ | pre-existing |
| `ragos user doctor` | `gar user doctor` | ✅ | pre-existing |
| `ragos user quota-sync` | `gar user quota-sync` | ✅ | pre-existing |
| `ragos user activity` | `gar user activity` | ✅ | pre-existing |
| `ragos server sync` | `gar server sync` | ✅ | pre-existing |
| `ragos server switch` | `gar server switch` | ✅ | pre-existing |
| `ragos server test` | `gar server test` | ✅ | pre-existing |
| `ragos server rollback` | `gar server rollback` | ✅ | pre-existing |
| `ragos server update` | `gar server update` | ✅ | pre-existing |
| `ragos server clean` | `gar server clean` | ✅ | pre-existing |
| `ragos server check` | `gar server check` | ✅ | pre-existing |
| `ragos server repl` | `gar server repl` | ✅ | pre-existing |
| `ragos server path` | `gar server path` | ✅ | pre-existing |
| `ragos server enter` | `gar server enter` | ✅ | pre-existing |
| `ragos server status` | `gar server status` | ✅ | pre-existing |
| `ragc switch` / `deploy` | `gar image build` (alias `deploy`) | ✅ | pre-existing |
| `ragc rollback` | `gar image rollback` | ✅ | pre-existing |
| `ragc list` / `ls` | `gar image list` | ✅ | pre-existing |
| `ragc status` | `gar image status` | ✅ | pre-existing |
| `ragc gc` | `gar image gc` | ✅ | pre-existing |
| `ragc doctor` | `gar image doctor` | ✅ | pre-existing |

## Migration plan

| Phase | Scope | Status | Commit count |
|---|---|---|---|
| 0 (Polish) | typed JSON structs in `user` subcommand | ✅ landed in `ff42060` | 1 |
| 1 | `gar group` (7 subcommands) + group_system skeleton | ✅ this release | 9 |
| 2.1 | `gar branding doctor` | ✅ this release | 1 |
| 2.2 | `gar client session-doctor` | ✅ this release | 1 |
| 3 | global polish (any remaining `serde_json::json!`, dead code) | ⏳ | 1 |
| 4 | remove `ragc/` and `server/ragos-cli.nix` from monorepo | ⏳ | 1 (monorepo) |

## Flake consumption

Once all phases land, the `garos` monorepo's `flake.nix` can drop
`ragc/` and `server/ragos-cli.nix` entirely and consume `gar` as a flake
input:

```nix
inputs.gar-cli = {
  url = "github:GARhq/gar";
  flake = true;
};

# environment.systemPackages
[ inputs.gar-cli.packages.${system}.default ]
```

Reference snapshot of pre-migration state is preserved at:

- Monorepo tag `pre-gar-migration` (commit `17f6cc7`)
- `garos/.migration-snapshots/ragos-cli.nix.snapshot`
- `garos/.migration-snapshots/ragc.snapshot/`
