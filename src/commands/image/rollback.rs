//! `gar image rollback` — revert to previous generation.
//!
//! Replaces `ragc rollback` (commands/rollback.sh, 231 LOC).
//! Full implementation in Fase 2 (round 17+).

use crate::cli::Channel;
use crate::error::Result;
use crate::output;

pub async fn run(_target: Option<String>, _channel: Option<Channel>) -> Result<()> {
    output::warn("gar image rollback — stub, implementação completa na Fase 2");
    output::info("Comportamento esperado:");
    output::info("  1. Validar current + previous ativos");
    output::info("  2. Swap de symlinks current ↔ previous (atômico)");
    output::info("  3. Validar boot coherence (iPXE ↔ kernel ↔ initrd)");
    output::info("  4. Atualizar manifest + gcroot");
    Ok(())
}