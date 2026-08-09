//! `gar image gc` — garbage collect old generations.
//!
//! Replaces `ragc gc` (commands/gc.sh, 151 LOC).
//! Full implementation in Fase 2.

use crate::error::Result;
use crate::output;

pub async fn run(_keep: Option<u32>) -> Result<()> {
    output::warn("gar image gc — stub, implementação completa na Fase 2");
    output::info("Comportamento esperado:");
    output::info("  1. Proteger ponteiros (current/previous/staged/rescue)");
    output::info("  2. Proteger recentes (grace_seconds)");
    output::info("  3. Proteger últimas N gerações com manifest");
    output::info("  4. Snapshot BTRFS prévio em /srv/data/snapshots/");
    output::info("  5. Remover gerações antigas");
    Ok(())
}