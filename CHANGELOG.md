## [v0.3.0] - 2026-10-15

### Added
- **cli(output)**: design system consistente para mensagens, tabelas, painéis e JSON, migrado para os comandos de server, image, client, user, group e branding.
- **server(update)**: espera pelo lock global por até 30 minutos, informa o tempo restante a cada 30 segundos e identifica o timeout ao expirar.

### Fixed
- **server(health)**: atualiza os serviços críticos para Caddy, dnsmasq, Redis/JuiceFS, MinIO e serviços de controle GAROS.
- **server(update)**: aborta antes de `nix flake update` se o checkout Git estiver sujo ou se não for possível verificar seu estado; sugere `gar server sync` ou commit/stash.
- **image(doctor)**: remove referências obsoletas a nginx e nfs-server dos health checks.
- **client(list)**: remove a mensagem stale de “Phase 0.5” do estado vazio do inventário.

### Refactored
- **server(update)**: descreve o pipeline por etapas declarativas compartilhadas pelo dry-run e pela execução.
- **image**: refatora build, GC, listagem/status e descoberta de gerações; separa a biblioteca Rust do entrypoint CLI.

---

## [v0.2.8] - 2026-10-14

### Refactored
- **refactor(commands/secrets)**: troca hex manual (`format!("{:02x}")`) por `hex::encode/decode`;
  remove `.expect("Failed to create encryptor")` por `.map_err`; extrai `output_encrypt_result` e
  `output_decrypt_result`; mensagens em português consistente com o restante do CLI.
- **refactor(commands/image/doctor)**: `run()` quebrada em `collect_infra_checks`,
  `collect_integrity_checks`, `tally` e `render` — 14 linhas no topo em vez de 134.
  `IMAGE_POINTERS`, `CHANNELS`, `CHANNEL_POINTER_PREFIXES` viram constantes nomeadas.
  `keys_dir()` unifica a resolução de `GAR_KEYS_DIR` (era duplicada em 2 checks).
- **refactor(services/client)**: extraídos `format_mac_colon()` e `parse_hex_octets()` —
  `build_magic_packet` agora aceita qualquer formato que `normalize_mac` aceita
  (não só canônico com `:`). `collect_report` usa `bump()` helper em vez de 6 linhas de if/else.
- **refactor(services/runtime_guard)**: `PLACEHOLDER_BOOT_DEVICES` vira constante (substitui
  match feio); sudo check agora valida exit code, não só ausência de erro;
  `.args(&args)` → `.args(args)` (clippy); usa `GarError` em vez de `crate::error::GarError`.
- **refactor(services/shell)**: `run_success` e `run_success_in_dir` consolidados em
  `run_inner(cwd: Option<&Path>)`; `SPINNER_TICKS` constante; `new_spinner()` e `failure()`
  helpers extraídos. Funções públicas viram one-liners que delegam.

### Changed
- **deps**: adicionado `hex = "0.4.3"` (usado por `commands/secrets`).

K-2026-10-14 / Onda 5 — DRY em shell, doctor, secrets, client, runtime_guard

---

## [v0.2.7] - 2026-10-14

### Refactored
- **refactor(services/boot)**: extraídos `render_active_ipxe()`, `render_empty_ipxe()` e
  `validate_ipxe_build_id()` — elimina 4 cópias do template iPXE e 8 repetições do
  padrão de validação de `build_id`. Sem mudança de comportamento.
- **refactor(services/build)**: extraídos `copy_artifact()`, `resolve_erofs_src()` e
  `sha256_of_optional_erofs()` — remove duplicação dos 3 blocos `cp -L` e do cálculo
  duplicado de SHA256 do EROFS.
- **refactor(services/channel)**: `target_hardware_class()` agora delega para
  `target_hardware_class_str()` — elimina match duplicado de mapeamento hardware.
- **refactor(services/branding)**: consolidados helpers SHA256; remove função de teste
  duplicada.
- **refactor(commands/server/lifecycle)**: `run_command()` unifica execução de
  `nix flake update`/`nix flake check`; `is_unit_present()` simplifica health check;
  `plan_only()` reescrito com tabela de etapas.
- **refactor(services/rollback)**: `remove_staged_link_if_stale()` e `parse_key_value()`
  removem duplicação de staged pointers e do parser KEY=VALUE.
- **refactor(commands/image/build)**: extraídos `discover_artifacts()`,
  `write_netboot_ipxe()`, `set_generation_permissions()`, `write_staged_manifest()`,
  `write_pending_rollback_if_needed()` e `atomic_symlink_swap()`. Função `run()`
  reduzida de ~360 para ~220 linhas.

### Fixed
- **style(gar)**: `cargo fmt` aplicado em todo o projeto.
- **refactor(gar)**: todos os warnings do `clippy --all-targets -- -D warnings`
  corrigidos.

K-2026-10-14 / Onda 4 — Clean code + DRY

---

## [v0.2.5] - 2026-10-12

### Fixed
- **build(gar)**: `gar server update` step [6/7] consolidava o bootloader
  com `nh os switch` — antes só logava e ia direto pro GC, então a nova
  geração NUNCA virava boot default. Bloqueador crítico: qualquer
  `gar server update` em prod mentia sobre persistência.
- **build(gar)**: `validation::validate_build_artifacts` agora confere
  que a chave pública em runtime (`keys/garos.pub`) bate byte-a-byte
  com a chave pinada (`<flake>/client/storage/garos.pub`). Antes passava
  no `--validate` mesmo se o operador rotacionou a chave mas esqueceu
  de comitar a nova pub — todo cliente existente rejeitava o manifest
  no boot (chave desconhecida).
- **tests(gar)**: race condition em `std::env::set_var/remove_var`
  causava flake 15/15 com `-j 4`. Adicionado `serial_test = "3"` e
  `#[serial(env_config)]` nos 10 tests que mexem em env (config,
  output, services/build, services/storage, services/validation,
  commands/image/doctor, commands/server/lifecycle). Agora 30/30
  estáveis com `-j 4`, 15/15 com `-j 8`.

### Added
- **cli(gar)**: `about=` em todas as variantes de `UserCmd` e
  `GroupCmd` que estavam sem — help mostrava linha em branco. Cobre
  add/resize/list/delete/doctor/quota-sync/activity/members/chmod/
  permissions/ensure-defaults.
- **tests(gar)**: 5 tests de regressão pros checks do doctor
  (manifest-checksum-kernel/initrd FAIL em divergência, public-key-match
  FAIL/PASS/missing-pinned). Garantem que se alguém regredir os checks
  implementados em v0.2.4, CI pega.

### Removed
- **chore(gar)**: deletado `src/commands/secrets_test.rs` (stub morto
  de 6 linhas com `Decryptor::new(...).unwrap()` sem `#[test]`, nunca
  chamado).

K-2026-10-12 / Onda 3 — Resiliência de build

---

## [v0.2.4] - 2026-10-12

### Added
- **feat(doctor)**: 5 checks novos a nível de artefato
  (manifest-checksum-kernel, manifest-checksum-initrd, manifest-signature,
  public-key-match, http-serve-content). Pega o modo de falha silencioso
  onde manifest + kernel + assinatura + chave "parecem" presentes mas
  são mutuamente inconsistentes — exatamente o que morderá na Phase C
  do rebuild GAROS.
- **refactor(main)**: extraído `setup_tracing()` + `dispatch()` em
  funções nomeadas; handler de Ctrl-C separado.

K-2026-10-06

---

## [v0.2.3] - 2026-10-12

### Added
- **feat(image build)**: flag `--validate` (default ON) — fail-fast em
  artefatos faltosos/corrompidos antes de retornar sucesso. Inclui
  checks de tamanho mínimo (bzImage/initrd > 1MB), recalcula SHA-256 e
  compara com manifest, verifica assinatura Ed25519.
- **feat(image build)**: orquestração em tmux opcional (`--tmux` /
  `--no-tmux` / `--tmux-session` / `--tmux-timeout`) — sobrevive a
  queda de SSH, recuperação via `tmux attach -t <session>`. Resolved
  via `resolve_tmux(tmux, no_tmux)` com regras TTY/CI/--json.
- **feat(image)**: extraído `match_image_cmd` puro (sem I/O) para
  testabilidade do dispatch.

K-2026-10-06

---

## [v0.2.2] - 2026-10-12

### Added
- **feat(server)**: `gar server update` agora roda `nix-collect-garbage`
  (preserva 7d) por padrão antes do disk precheck. Idempotente com
  `nix.gc.automatic` no NixOS module. Opt-out via `--skip-gc`.
- **feat(server check)**: flag `--flake-drift` compara o rev do
  `gar-cli` no `flake.lock` local contra o HEAD de `GARhq/gar`. Exit
  não-zero se drift detectado, para wire em cron/monitoramento.
  K-2026-10-06: previne o modo "lockfile semanas defasado" silencioso.
- **feat(server update)**: flags `--services` (override critical
  services list), `--skip-disk-check`, `--skip-health-check`, `--dry-run`.

K-2026-10-06

---

## [v0.2.1] - 2026-10-03

- feat(server): port garos-update safety flow with new flags (dry-run, services, skip-disk-check, skip-health-check)

---

## [v0.2.0] - 2026-10-02

- feat: devShell auto flake update on stale lock

---

# Changelog

## [v0.1.0] - 2026-10-01

- feat: add SemVer release tooling and versioning policy
