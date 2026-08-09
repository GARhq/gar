//! `gar image doctor` — health check.
//!
//! Replaces `ragc doctor` (commands/doctor.sh, 208 LOC).
//! Full implementation in Fase 2.

use crate::error::Result;
use crate::output;

pub async fn run() -> Result<()> {
    output::warn("gar image doctor — stub, implementação completa na Fase 2");
    output::info("Comportamento esperado:");
    output::info("  1. Checar dnsmasq/nginx/NFS");
    output::info("  2. Checar tier1 mount (BTRFS)");
    output::info("  3. Checar symlinks (current/previous/staged/rescue)");
    output::info("  4. Checar arquivos (bzImage/initrd/manifest.json)");
    output::info("  5. Validar coherence (boot.ipxe ↔ kernel)");
    Ok(())
}