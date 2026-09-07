# services

## 📌 Visão Geral
Módulo integrante da arquitetura do repositório GAROS no caminho `gar/src/services`.

## ⚙️ O que esta pasta faz
- Fornecer a lógica executável de backend, ferramentas CLI ou componentes de alta performance em Rust.

## 📂 Conteúdo e Arquivos Detalhados
- `atomic_file.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo funções `write_atomic`.
- `atomic_path.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo funções `atomic_symlink, atomic_remove_path, tmp_sibling`.
- `boot.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `BootState` e enums `Coherence` e funções `is_empty, into_result, ipxe_declared_value`.
- `branding.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `BrandingCheck, BrandingReport, SurfaceEntry` e funções `parse_baseline_manifest, parse_baseline_manifest_str`.
- `btrfs.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo funções `create_subvolume, snapshot_readonly, set_quota`.
- `build.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `BuildArtifact` e funções `build_or_reuse_system, compute_build_id, sha256_file`.
- `channel.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo enums `instead` e funções `target_channel, target_hardware_class_str, target_hardware_class`.
- `client.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `ClientManifest, ClientSessionReport, ClientRecord` e enums `ClientStatus` e funções `current_manifest, nfs_exports, inventory_text`.
- `filesystem.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo enums `FsType, FsOps` e funções `from_str, as_str, supports_quota`.
- `generation.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo enums `for, RuntimeSource` e funções `read_generation_init_path, read_generation_kernel_params, runtime_source_from_params_file`.
- `generations.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo funções `current_number, clean_fallback, test_current_number_returns_string`.
- `git.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo funções `sync_full`.
- `group_system.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `QuotaSpec, GroupMeta, GroupRow` e funções `new, load, save`.
- `lock.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `Owner` e enums `AcquireOutcome` e funções `current, format, open_lock_file`.
- `manifest.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `Manifest, Artifacts, Checksums` e enums `Status` e funções `read, write, set_status`.
- `mod.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust com lógica de execução de sistema em Rust.
- `nix.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo funções `flake_update, flake_check, flake_repl`.
- `nixos_rebuild.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo funções `switch, test, rollback`.
- `rollback.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `RollbackRecord` e enums `RollbackLoadOutcome` e funções `into_option, is_loaded, write_pending_rollback`.
- `runtime_guard.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo funções `validate, require_flake_dir, require_runtime_file`.
- `shell.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo funções `run_success, run_success_in_dir, exec_in_dir`.
- `storage.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `ScopedEnv` e funções `storage_mount_ready, skip_storage_checks, ensure_tier1_ready`.
- `user_system.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `ClientUserEntry, ClientUsersCatalog, UserGroupsCatalog` e funções `useradd_system, useradd_to_group, userdel_from_group`.

## 🔄 Histórico de Mudanças Comportamentais
- **[Inicial]**: Mapeamento e documentação detalhada da estrutura inicial do módulo.
