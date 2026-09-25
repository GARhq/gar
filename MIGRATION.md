# garos → gar migration

Mapping from the legacy Nix-based `garos` + `ragc` CLIs to the unified
Rust-based `gar` CLI. Updated as each Phase is implemented and merged.

## Status legend

| Symbol | Meaning |
|---|---|
| ✅ | implemented + tested in `gar` |
| ⏳ | pending migration |
| ❌ | intentionally not migrated (infra-only, lives in Nix modules) |

## Command mapping

| garos / ragc command | gar command | Status | Phase |
|---|---|---|---|
| `garos group add` | `gar group add` | ✅ | Phase 1 |
| `garos group list` | `gar group list` | ✅ | Phase 1 |
| `garos group delete` | `gar group delete` | ✅ | Phase 1 |
| `garos group chmod` | `gar group chmod` | ✅ | Phase 1 |
| `garos group members` | `gar group members` | ✅ | Phase 1 |
| `garos group permissions` | `gar group permissions` | ✅ | Phase 1 |
| `garos group ensure-defaults` | `gar group ensure-defaults` | ✅ | Phase 1 |
| `garos branding doctor` | `gar branding doctor` | ✅ | Phase 2.1 |
| `garos client session-doctor` | `gar client session-doctor` | ✅ | Phase 2.2 |
| `garos user add` | `gar user add` | ✅ | pre-existing |
| `garos user resize` | `gar user resize` | ✅ | pre-existing |
| `garos user list` | `gar user list` | ✅ | pre-existing |
| `garos user delete` | `gar user delete` | ✅ | pre-existing |
| `garos user doctor` | `gar user doctor` | ✅ | pre-existing |
| `garos user quota-sync` | `gar user quota-sync` | ✅ | pre-existing |
| `garos user activity` | `gar user activity` | ✅ | pre-existing |
| `garos server sync` | `gar server sync` | ✅ | pre-existing |
| `garos server switch` | `gar server switch` | ✅ | pre-existing |
| `garos server test` | `gar server test` | ✅ | pre-existing |
| `garos server rollback` | `gar server rollback` | ✅ | pre-existing |
| `garos server update` | `gar server update` | ✅ | pre-existing |
| `garos server clean` | `gar server clean` | ✅ | pre-existing |
| `garos server check` | `gar server check` | ✅ | pre-existing |
| `garos server repl` | `gar server repl` | ✅ | pre-existing |
| `garos server path` | `gar server path` | ✅ | pre-existing |
| `garos server enter` | `gar server enter` | ✅ | pre-existing |
| `garos server status` | `gar server status` | ✅ | pre-existing |
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
| 4 | remove `ragc/` and `server/garos-cli.nix` from monorepo | ⏳ | 1 (monorepo) |

## Flake consumption

Once all phases land, the `garos` monorepo's `flake.nix` can drop
`ragc/` and `server/garos-cli.nix` entirely and consume `gar` as a flake
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
- `garos/.migration-snapshots/garos-cli.nix.snapshot`
- `garos/.migration-snapshots/ragc.snapshot/`
