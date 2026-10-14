# AGENTS.md — gar (GAROS Unified CLI)

## Identidade e Escopo

Este repositório contém o `gar`, a CLI unificada do ecossistema GAROS escrita em **Rust**.

- **Função:** administrar host NixOS (`gar server`), imagens diskless (`gar image`), identidade Unix (`gar user`, `gar group`), provisionamento BTRFS (`gar provision-home`) e branding (`gar branding`).
- **Stack:** Rust 1.80+, Cargo, Clap, Tokio, Serde, anyhow/thiserror, Nix Flake.
- **Critério de qualidade:** `cargo test` e `cargo clippy -- -D warnings` devem passar antes de qualquer hand-off.

## Workflow de Desenvolvimento com Skills

Siga este pipeline ao trabalhar no `gar`. Cada skill é invocada pelo seu `skill_view("<nome>")` ou, se já estiver carregada, seguindo suas instruções.

| Etapa | Skill | Quando usar |
|-------|-------|-------------|
| 1. Clarear requisito | `grill-me` | Sempre que uma nova feature, bugfix ou comando for solicitado. A necessidade deve estar cristalina antes de qualquer linha de código. |
| 2. Implementar | `aura-apex` | Implementação com testes, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` e build de release quando apropriado. |
| 3. Design de saída | `aura-impeccable` | Quando o comando produz tabelas, JSON, mensagens de erro ou `--help` — garantir consistência visual e clareza. |
| 4. Micro-polish | `aura-interface-polish` | Refinar formatação de saída, alinhamento de colunas, cores (`owo-colors`), mensagens de progresso (`indicatif`) e estados de loading. |
| 5. Auditoria de qualidade | `aura-thermo-review` | Antes de abrir PR ou criar tag: verificar tamanho de arquivos, acoplamento, tratamento de erros e cobertura de testes. |

## Comandos Obrigatórios de Verificação

Antes de considerar qualquer tarefa concluída:

```bash
# Formatação
cargo fmt --all -- --check

# Linter
cargo clippy --all-targets -- -D warnings

# Testes
cargo test

# Build de release (para validar otimização e warnings finais)
cargo build --release

# Verificação Nix (quando o flake for tocado)
nix flake check
```

## Regras do Projeto

- **Sem `unwrap`/`expect` novos** sem justificativa documentada. Preferir `anyhow::Context` e `thiserror`.
- **Erros amigáveis:** mensagens devem indicar o que falou, por quê e o que fazer.
- **Saída estruturada:** todo novo comando deve suportar `--json` quando fizer sentido operacional.
- **Commits:** Conventional Commits, um arquivo/por commit, tag `vX.Y.Z` anotada e changelog atualizado.
- **Sem scope creep:** o `gar` é uma CLI; lógica de negócio pesada pertence a outros serviços.

## Estrutura de Referência Rápida

```
src/
├── main.rs       # entrypoint e parser Clap
├── cli.rs        # estruturas de subcomandos
├── config.rs     # resolução de config
├── error.rs      # tipos de erro
├── output.rs     # formatadores de saída
├── commands/     # subcomandos
└── services/     # wrappers de Nix, BTRFS, Systemd, PAM
```

## Integração com o Ecossistema

- Alterações em `gar server` afetam o host `srv-garos` (`10.10.10.254`).
- Alterações em `gar image` afetam clients diskless; sempre rodar `gar image doctor` após mudanças.
- Alterações em `gar user`/`gar group` alteram o SSOT de identidade em `server/identity/`.

> **Regra de segurança:** nunca logar tokens, chaves age ou hashes de senha no chat. Secrets só via `age` e arquivos fora do repo.
