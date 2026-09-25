# ⚡ gar — GAROS Unified CLI

> **Ferramenta de linha de comando unificada em Rust para administração, gestão de imagens, inventário, provisionamento e usuários do ecossistema GAROS.**

[![Rust](https://img.shields.io/badge/Rust-1.80+-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Cargo Tests](https://img.shields.io/badge/Tests-242%20Passing-success.svg)](#-testes-e-qualidade)
[![Status](https://img.shields.io/badge/Phase-6.1--6.2%20Complete-brightgreen.svg)](#-migra%C3%A7%C3%A3o-e-hist%C3%B3rico)
[![License](https://img.shields.io/badge/License-Proprietary-red.svg)](#)

---

## 📌 Visão Geral

O `gar` é a interface de administração unificada do ecossistema GAROS escrita em Rust. Ele substituiu com sucesso as ferramentas legadas em bash (`ragc` e `garos`), consolidando toda a gestão operacional sob uma única CLI compilada e extremamente rápida, com suporte a saída estruturada (JSON / Tabelas), validações determinísticas e tratamento seguro de erros via `anyhow` e `thiserror`.

### Principais Capacidades
- 🖥️ **`gar server`**: Administração do host NixOS (`srv-garos`), executando `switch`, `test`, `rollback`, `update`, `clean` e diagnósticos de saúde.
- 🖼️ **`gar image`**: Compilação e ciclo de vida de imagens diskless (`build`, `list`, `status`, `rollback`, `gc`, `doctor`).
- 👤 **`gar user` & `gar group`**: Operações no SSOT de Identidade (`server/identity/`), gerando receitas declarativas Nix e sincronizando BTRFS homes.
- 💾 **`gar provision-home`**: Provisionamento atômico em Rust de subvolumes BTRFS com configuração de quotas e permissões `0700`.
- 🎨 **`gar branding`**: Personalização de temas GRUB, Plymouth, SDDM e artefatos de marca para a infraestrutura.

---

## 🏗️ Estrutura do Código Rust

```text
gar/
├── Cargo.toml               # Manifesto Cargo com dependências (clap, tokio, serde, anyhow, tracing)
├── Cargo.lock               # Árvore determinística de dependências Rust
├── default.nix              # Expressão de build declarativo Nix para a CLI
├── flake.nix                # Input Flake exportando o pacote `gar`
├── MIGRATION.md             # Documento de transição histórica garos/ragc -> gar
├── src/
│   ├── main.rs              # Ponto de entrada CLI e parser de argumentos
│   ├── cli.rs               # Definição das estruturas Clap (Commands, Subcommands & Flags)
│   ├── config.rs            # Resolução de configurações (~/.config/gar/config.toml)
│   ├── error.rs             # Tipos de erro e tratamentos formatados
│   ├── output.rs            # Formatadores de saída (Tabela human-readable e JSON)
│   ├── commands/            # Módulos dos subcomandos
│   │   ├── server.rs        # nixos-rebuild wrapper e gerenciamento do servidor
│   │   ├── client.rs        # Diagnósticos e comandos de clientes diskless
│   │   ├── image/           # Build, list e publicação de imagens netboot
│   │   ├── user/            # Gerenciamento SSOT de usuários Unix
│   │   ├── group/           # Gerenciamento SSOT de grupos
│   │   ├── provision_home.rs# Provisionamento de subvolumes BTRFS
│   │   └── branding/        # Instalação de temas visuais
│   └── services/            # Camada de serviços internos (Nix, BTRFS, Systemd, PAM)
└── tests/                   # Testes de integração em Rust
```

---

## 💻 Guia Rápido de Uso

### 1. Administração do Servidor NixOS (`gar server`)

```bash
# Ver status do servidor e geração ativa do NixOS
gar server status

# Aplicar nova configuração declarativa do srv-garos
sudo gar server switch

# Testar configuração sem persistir no bootloader
sudo gar server test

# Reverter para a geração anterior
sudo gar server rollback

# Atualizar flake inputs (nix flake update) e aplicar switch
sudo gar server update
```

### 2. Gestão de Imagens do Cliente Diskless (`gar image`)

```bash
# Compilar nova imagem diskless para o perfil desktop-generic
sudo gar image build --target desktop-generic --channel generic

# Listar todas as imagens registradas e armazenadas em /srv/garos/images/
gar image list

# Exibir status de saúde e uso de disco das imagens
gar image status

# Limpar imagens antigas mantendo as últimas 5 gerações
sudo gar image gc --keep 5
```

### 3. Gestão de Usuários e Homes BTRFS (`gar user` / `gar provision-home`)

```bash
# Listar usuários cadastrados no SSOT
gar user list

# Formato JSON estruturado para automação
gar user list --json

# Provisionar manualmente home BTRFS com quota
sudo gar provision-home --user garton --quota 50G
```

---

## 🧪 Compilação e Testes

Para compilar e rodar a suíte de testes de integração:

```bash
# Compilar o binário em modo debug
cargo build

# Compilar em modo release otimizado
cargo build --release

# Executar a suíte de 242+ testes unitários e de integração
cargo test

# Executar o linter oficial Clippy
cargo clippy -- -D warnings
```

Via Nix:

```bash
# Compilar via Flake
nix build .#gar

# Verificar o flake
nix flake check
```

---

## 🔄 Migração e Histórico (garos / ragc → gar)

O `gar` consolida todas as funções operacionais anteriores:
- **`garos`** → Migrado para `gar server` e `gar image`.
- **`ragc`** → Migrado para `gar user`, `gar group` e `gar inventory`.

Para mais detalhes sobre a especificação técnica de migração, consulte o arquivo [`MIGRATION.md`](./MIGRATION.md).

---

## 📄 Veja Também

- 📘 [GAROS Core Monorepo](file:///home/garton/Projetos/garos-dev/GAROS/README.md)
- 💿 [GAROS Installer Repository](file:///home/garton/Projetos/garos-dev/GAROSInstaller/README.md)
- 🧠 [Decisões Arquiteturais no Vault](file:///home/garton/Projetos/garos-dev/garos-think-vault/README.md)
