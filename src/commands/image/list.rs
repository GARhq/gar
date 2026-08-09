//! `gar image list` — list all generations.
//!
//! Replaces `ragc list` (commands/list.sh, 59 LOC).
//! Full implementation in Fase 2.

use crate::error::Result;
use crate::output;

pub async fn run() -> Result<()> {
    output::warn("gar image list — stub, implementação completa na Fase 2");
    output::info("Comportamento esperado:");
    output::info("  1. Listar todos os diretórios v* em images_root");
    output::info("  2. Ler metadata de cada manifest.json");
    output::info("  3. Mostrar tabela: versão | tamanho | timestamp | target | canal | status");
    Ok(())
}