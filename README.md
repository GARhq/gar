# gar

## 📌 Visão Geral
CLI oficial em Rust (`gar`) para administração, gestão de clientes, servidores, imagens e operações do ecossistema GAROS.

## ⚙️ O que esta pasta faz
- Gerenciar especificações declarativas do NixOS, módulos do sistema e receitas de build.
- Manter documentadas as decisões de design, especificações de rotas e guias operacionais.

## 📂 Conteúdo e Arquivos Detalhados
- `docs/`: **[Módulo]** — Subsistema e arquivos de organização do módulo `docs`.
- `src/`: **[Módulo]** — Código-fonte principal da CLI GAROS, organizado por comandos e serviços.
- `tests/`: **[Módulo]** — Subsistema e arquivos de organização do módulo `tests`.
- `Cargo.lock`: **[Trava de Versões Cargo]** — Arquivo de trava determinístico contendo a árvore completa de crates e versões compiladas em Rust.
- `Cargo.toml`: **[Manifesto Cargo Rust]** — Especifica metadados do pacote Rust, dependências (description, license, repository, readme) e perfis de compilação.
- `MIGRATION.md`: **[Documentação em Markdown]** — Documento de especificação técnica abordando *ragos → gar migration*.
- `default.nix`: **[Módulo NixOS]** — Implementa a especificação declarativa do sistema (expressão declarativa de configuração NixOS).
- `flake.lock`: **[Trava de Dependências Flake]** — Registro determinístico com hashes e revisões exatas dos repositórios e módulos importados pelo Flake.
- `flake.nix`: **[Nix Flake Principal]** — Define as entradas (gar-cli, nixpkgs, flake-utils) e configurações de sistema () do ecossistema Nix.

## 🔄 Histórico de Mudanças Comportamentais
- **[Inicial]**: Mapeamento e documentação detalhada da estrutura inicial do módulo.
