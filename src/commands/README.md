# commands

## 📌 Visão Geral
Implementação técnica de todos os subcomandos da CLI (`branding`, `client`, `group`, `image`, `server`, `user`).

## ⚙️ O que esta pasta faz
- Fornecer a lógica executável de backend, ferramentas CLI ou componentes de alta performance em Rust.

## 📂 Conteúdo e Arquivos Detalhados
- `branding/`: **[Módulo]** — Subsistema e arquivos de organização do módulo `branding`.
- `client/`: **[Módulo]** — Subsistema e arquivos de organização do módulo `client`.
- `group/`: **[Módulo]** — Subsistema e arquivos de organização do módulo `group`.
- `image/`: **[Módulo]** — Subsistema e arquivos de organização do módulo `image`.
- `server/`: **[Módulo]** — Subsistema e arquivos de organização do módulo `server`.
- `user/`: **[Módulo]** — Subsistema e arquivos de organização do módulo `user`.
- `branding.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `DoctorFlags, BrandingSummary, fields` e enums `carries` e funções `dispatch, cmd_doctor, render_check`.
- `client.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `ClientSessionSummary, WakeResult` e funções `dispatch, cmd_session_doctor, cmd_list`.
- `group.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `GroupAddResult, GroupDeleteResult, GroupMembersResult` e funções `dispatch, cmd_add, report_existing`.
- `mod.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust com lógica de execução de sistema em Rust.
- `server.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `StatusReport` e funções `dispatch, cmd_sync, cmd_switch`.
- `user.rs`: **[Código-Fonte Rust]** — Módulo de alta performance em Rust, contendo structs `AddResult` e funções `dispatch, cmd_add, cmd_resize`.

## 🔄 Histórico de Mudanças Comportamentais
- **[Inicial]**: Mapeamento e documentação detalhada da estrutura inicial do módulo.
