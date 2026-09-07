# src

## 📌 Visão Geral
Código-fonte principal da CLI GAROS, organizado por comandos e serviços.

## ⚙️ O que esta pasta faz
- Fornecer a lógica executável de backend, ferramentas CLI ou componentes de alta performance em Rust.

## 📂 Conteúdo e Arquivos Detalhados
- `commands/`: **[Módulo]** — Implementação técnica de todos os subcomandos da CLI (`branding`, `client`, `group`, `image`, `server`, `user`).
- `services/`: **[Módulo]** — Subsistema e arquivos de organização do módulo `services`.
- `cli.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `Cli` e enums `Command, ImageCmd`.
- `config.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `Config` e funções `from_env, installable, env_string`.
- `error.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo enums `GarError` e funções `config, invalid_argument, validation`.
- `main.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo funções `main, exit_with`.
- `output.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo enums `OutputMode` e funções `from_env, is_json, ok`.

## 🔄 Histórico de Mudanças Comportamentais
- **[Inicial]**: Mapeamento e documentação detalhada da estrutura inicial do módulo.
