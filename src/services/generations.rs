//! Nix generation management (current generation + GC).
//!
//! Replaces `current_generation` + `clean_without_nh` from ragos-cli.nix.

use std::process::Command;

use crate::config::Config;
use crate::error::Result;

/// Get the current NixOS system generation number.
pub fn current_number() -> String {
    let output = Command::new("readlink")
        .arg("/nix/var/nix/profiles/system")
        .output()
        .ok();
    match output {
        Some(o) if o.status.success() => {
            let target = String::from_utf8_lossy(&o.stdout);
            // Pattern: system-<N>-link
            if let Some(idx) = target.find("system-") {
                let after = &target[idx + 7..];
                let end = after.find('-').unwrap_or(after.len());
                return after[..end].to_string();
            }
            "desconhecida".into()
        }
        _ => "desconhecida".into(),
    }
}

/// Clean old generations, keeping N most recent or those within KEEP_SINCE.
pub fn clean_fallback(cfg: &Config) -> Result<()> {
    let keep_n = 5u32;
    let keep_since = "7d";
    let keep_since_date = "7 days ago";

    // List generations
    let output = Command::new("nix-env")
        .args(["--list-generations", "--profile", "/nix/var/nix/profiles/system"])
        .output()?;
    if !output.status.success() {
        return Err(crate::error::GarError::config(format!(
            "nix-env list-generations falhou: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut generations: Vec<(u32, String)> = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        if let Ok(n) = parts[0].parse::<u32>() {
            let stamp = parts.get(1..3).map(|p| p.join(" ")).unwrap_or_default();
            generations.push((n, stamp));
        }
    }

    let total = generations.len();
    let keep_start = (total as u32).saturating_sub(keep_n) as usize;

    // Cutoff epoch
    let cutoff_epoch = Command::new("date")
        .args(["-d", keep_since_date, "+%s"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8_lossy(&o.stdout).trim().parse::<i64>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    let mut deletable: Vec<u32> = Vec::new();
    for (i, (gen, stamp)) in generations.iter().enumerate() {
        if i < keep_start {
            if let Ok(epoch) = Command::new("date")
                .args(["-d", stamp, "+%s"])
                .output()
                .map(|o| {
                    if o.status.success() {
                        String::from_utf8_lossy(&o.stdout)
                            .trim()
                            .parse::<i64>()
                            .unwrap_or(0)
                    } else {
                        0
                    }
                })
            {
                if epoch > 0 && epoch < cutoff_epoch {
                    deletable.push(*gen);
                }
            }
        }
    }

    if !deletable.is_empty() {
        let gen_refs: Vec<String> = deletable.iter().map(|g| g.to_string()).collect();
        let _ = Command::new("nix-env")
            .args(["--profile", "/nix/var/nix/profiles/system", "--delete-generations"])
            .args(&gen_refs)
            .status()?;
    }

    let _ = Command::new("nix-collect-garbage")
        .args(["--delete-older-than", keep_since])
        .status();

    if Command::new("which")
        .arg("nix-store")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        let _ = Command::new("nix-store").arg("--optimise").status();
    }

    // suppress unused warning for cfg
    let _ = cfg;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_number_returns_string() {
        let s = current_number();
        assert!(!s.is_empty());
        // Could be "desconhecida" or a number string
        assert!(s.parse::<u32>().is_ok() || s == "desconhecida");
    }
}