//! `gar branding` subcommand — branding diagnostics.

use owo_colors::OwoColorize;
use serde::Serialize;

use crate::cli::BrandingCmd;
use crate::config::Config;
use crate::error::Result;
use crate::output;
use crate::services::branding::{self, BrandingReport};

pub async fn dispatch(cmd: BrandingCmd) -> Result<()> {
    match cmd {
        BrandingCmd::Doctor => cmd_doctor().await,
    }
}

#[derive(Debug, Serialize)]
struct BrandingSummary {
    report: BrandingReport,
    healthy: bool,
}

pub async fn cmd_doctor() -> Result<()> {
    let cfg = Config::from_env()?;
    let report = branding::collect_report();
    let healthy = report.fail_count == 0;

    if cfg.json_output {
        output::json(&BrandingSummary {
            report,
            healthy,
        })?;
    } else {
        output::section("GAR Branding Doctor");
        println!();
        render_check(&report.sddm_current);
        render_check(&report.sddm_theme_dirs);
        render_check(&report.plymouth_theme);
        render_check(&report.plasma_look_and_feel);
        render_check(&report.plasma_desktoptheme);
        render_check(&report.plasma_color_schemes);
        render_check(&report.plasma_wallpapers);
        println!();
        if healthy {
            output::ok(format!(
                "branding OK ({}/{} checks passed)",
                report.ok_count,
                report.ok_count + report.fail_count
            ));
        } else {
            output::err(format!(
                "branding com gaps ({}/{} checks OK, {} missing)",
                report.ok_count,
                report.ok_count + report.fail_count,
                report.fail_count
            ));
        }
    }
    Ok(())
}

fn render_check(c: &branding::BrandingCheck) {
    let status = if c.found && !c.matches.is_empty() && c.matches[0] != "nao encontrado" {
        format!("{}", "OK".green().bold())
    } else {
        format!("{}", "GAP".yellow().bold())
    };
    println!("  [{}] {}", status, c.label.bold());
    println!("        path:    {}", c.path);
    if c.matches.is_empty() {
        println!("        matches: (vazio)");
    } else if c.matches.len() == 1 {
        println!("        matches: {}", c.matches[0]);
    } else {
        println!("        matches:");
        for m in &c.matches {
            println!("          - {}", m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_serializes_with_healthy_flag() {
        let r = branding::collect_report();
        let healthy = r.fail_count == 0;
        let json = serde_json::to_string(&BrandingSummary {
            report: r,
            healthy,
        })
        .unwrap();
        assert!(json.contains("\"healthy\""));
        assert!(json.contains("\"ok_count\""));
    }

    #[test]
    fn test_branding_summary_shape() {
        let r = branding::collect_report();
        let s = BrandingSummary {
            report: r,
            healthy: false,
        };
        let v: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert!(v.get("healthy").is_some());
        assert!(v.get("report").is_some());
        assert!(v.get("report").unwrap().get("sddm_current").is_some());
    }

    #[test]
    fn test_branding_report_total_is_seven() {
        let r = branding::collect_report();
        assert_eq!(r.ok_count + r.fail_count, 7);
    }
}
