//! `gar image status` — show active generation status.
//!
//! Replaces `ragc status` (commands/status.sh, 74 LOC).
//! Full implementation in Fase 2.

use crate::error::Result;
use crate::output;

pub async fn run() -> Result<()> {
    output::warn("gar image status — stub, implementação completa na Fase 2");
    output::info("Comportamento esperado:");
    output::info("  1. Mostrar geração ativa (current)");
    output::info("  2. Mostrar geração anterior (previous)");
    output::info("  3. Mostrar rescue, staged");
    output::info("  4. Mostrar canais paralelos (generic/lab/rescue)");
    output::info("  5. URLs kernel/initrd/iPXE");
    Ok(())
}