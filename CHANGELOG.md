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
