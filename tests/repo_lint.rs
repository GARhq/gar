// K-130R-3 (2026-09-07): rewrote repo_lint.rs to validate the **owning**
// crate's repo (`gar/`), not the sibling `garos/` monorepo.
//
// The original implementation (commit 1ee1618) walked the filesystem
// looking for a sibling `garos/` checkout and ran `docs_layout`,
// `script_headers`, `inventory_rules` etc against THAT tree. This had two
// problems:
//
//   1. Ownership: a Rust crate should never lint a different repo's
//      structure. The lint suite belongs in `garos/tests/` (where the
//      docs/scripts layout actually lives).
//   2. CI portability: when the binary is shipped to production via
//      `garos/flake.nix#gar-cli.packages.x86_64-linux.default`, the sibling
//      `garos/` checkout is not present on disk — the helper panics
//      (line 23) and breaks `cargo test --workspace` in CI.
//
// The fix is structural: this suite now validates `gar/` itself, where
// the structure invariants actually live (src/ tree, no banned legacy
// strings in markdown, no stray temp artifacts, Cargo.toml sanity).

use std::fs;
use std::path::{Path, PathBuf};

// Resolve the owning crate's repo root. `cargo test` runs from
// `tests/repo_lint.rs` with `CARGO_MANIFEST_DIR` set to the crate root,
// so this is deterministic — no filesystem guessing.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn list_files_in_dir(base: &Path, max_depth: Option<usize>, recursive: bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs_to_visit = vec![(base.to_path_buf(), 0)];

    while let Some((dir, depth)) = dirs_to_visit.pop() {
        if let Some(max) = max_depth {
            if depth > max {
                continue;
            }
        }

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();

                // Skip common noise directories.
                if file_name == ".git"
                    || file_name == "target"
                    || file_name == "node_modules"
                    || file_name == ".direnv"
                {
                    continue;
                }

                if path.is_file() {
                    files.push(path);
                } else if path.is_dir() && recursive {
                    dirs_to_visit.push((path, depth + 1));
                }
            }
        }
    }
    files
}

#[test]
fn test_root_markdown_allowlist() {
    // `gar/` is a Rust crate, not a docs monorepo. The only top-level
    // markdown that should exist here is `README.md` (Cargo's `readme`
    // field) and `MIGRATION.md` (the ragos→gar migration record) —
    // everything else belongs in `docs/` or is an intentional file we
    // list explicitly below.
    let root = repo_root();
    let allowed_mds = vec!["README.md", "MIGRATION.md"];

    let files = list_files_in_dir(&root, Some(1), false);
    for file in files {
        if let Some(ext) = file.extension() {
            if ext == "md" {
                let filename = file.file_name().unwrap().to_string_lossy();
                assert!(
                    allowed_mds.contains(&filename.as_ref()),
                    "Markdown no topo do crate fora da allowlist: {} \
                     (use `docs/` para documentação adicional)",
                    filename
                );
            }
        }
    }
}

#[test]
fn test_no_banned_legacy_references_in_runtime_strings() {
    // Drift guards: NOTHING in this crate should reference the legacy
    // `ragos`/`ragc` vocabulary in PRODUCTION runtime strings — Nix
    // installable refs, kernel cmdline tokens, env-var names, format
    // strings, and JSON field names. Historical mentions in docstrings
    // and migration notes (MIGRATION.md, comments referencing the past)
    // are tolerated; the lint only fires on text that ends up in
    // produced files (iPXE bundles, Nix installables, manifests).
    //
    // Detection strategy: strip // line comments and /* */ block
    // comments before scanning. That way the lint can reference the
    // banned tokens in its OWN diagnostics without self-triggering.
    let root = repo_root();

    let banned_substrings = vec![
        // Kernel cmdline token — drifted back to `garos` in K-130R-2.
        "ragos.primaryNicMac=",
        // Nix installable attribute — K-130R-1.
        "nixosConfigurations.ragos-client-",
        "system.build.ragosPublishTree",
    ];

    let src_dir = root.join("src");
    if !src_dir.exists() {
        return;
    }
    let files = list_files_in_dir(&src_dir, None, true);
    for file in files {
        if file.extension().unwrap_or_default() != "rs" {
            continue;
        }
        let raw = fs::read_to_string(&file).expect("read src .rs file");
        // Strip comments to avoid self-trigger on diagnostic text.
        let stripped = strip_rust_comments(&raw);
        for needle in &banned_substrings {
            if stripped.contains(needle) {
                panic!(
                    "src/{:?} contains banned legacy reference `{}` in runtime code — \
                     K-130R requires `ragos`/`ragc` be replaced by \
                     `gar`/`garos` in production code paths",
                    file.file_name().unwrap(),
                    needle
                );
            }
        }
    }
}

/// Strip // line comments and /* block comments */ from Rust source.
///
/// Crude but sufficient for lint purposes — we don't need full token
/// awareness, only to neutralize self-referential diagnostics and
/// historical "this used to be ragos" comments.
fn strip_rust_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    // Track `#[cfg(test)]` blocks so we can exclude them from the lint
    // scan — tests legitimately reference the BANNED form to assert it
    // is NOT present (negative contains checks).
    let mut in_cfg_test = false;
    let mut brace_depth: i32 = 0;
    while i < bytes.len() {
        // Detect `#[cfg(test)]` (or `#[cfg(all(test, ...))]`) at item
        // boundary and mark the next block as test-only.
        if bytes[i] == b'#' && i + 1 < bytes.len() && bytes[i + 1] == b'[' && !in_cfg_test {
            // Scan forward to find a balanced `]`.
            let mut j = i + 2;
            let mut depth = 1i32;
            while j < bytes.len() && depth > 0 {
                if bytes[j] == b'[' {
                    depth += 1;
                } else if bytes[j] == b']' {
                    depth -= 1;
                }
                j += 1;
            }
            let attr_text = std::str::from_utf8(&bytes[i..j]).unwrap_or("");
            if attr_text.contains("cfg(test") || attr_text.contains("cfg(all(test") {
                in_cfg_test = true;
                brace_depth = 0;
            }
            // Replace the attribute with whitespace so byte offsets
            // remain roughly aligned.
            for _ in 0..(j - i) {
                out.push(' ');
            }
            i = j;
            continue;
        }

        // Track braces while inside a #[cfg(test)] block.
        if in_cfg_test {
            if bytes[i] == b'{' {
                brace_depth += 1;
            } else if bytes[i] == b'}' {
                brace_depth -= 1;
                if brace_depth == 0 {
                    in_cfg_test = false;
                }
            }
            out.push(' ');
            i += 1;
            continue;
        }

        // Block comment.
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            out.push(' ');
            continue;
        }
        // Line comment.
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            // Keep the newline so byte offsets roughly align.
            continue;
        }
        // String literal — keep as-is (we want to catch banned tokens
        // inside format! strings and error messages).
        if bytes[i] == b'"' {
            out.push('"');
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    out.push(bytes[i] as char);
                    out.push(bytes[i + 1] as char);
                    i += 2;
                } else {
                    out.push(bytes[i] as char);
                    i += 1;
                }
            }
            if i < bytes.len() {
                out.push('"');
                i += 1;
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[test]
fn test_no_temporary_artifacts() {
    // Hygiene rule: no stray `result*`, `*.log`, `*.tmp`, `*.bak`, or
    // `nohup.out` files in the active checkout.
    let root = repo_root();
    let files = list_files_in_dir(&root, None, true);

    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        if name == "result"
            || name.starts_with("result-")
            || name.ends_with(".log")
            || name.ends_with(".tmp")
            || name.ends_with(".bak")
            || name == "nohup.out"
        {
            if !file.to_string_lossy().contains(".git") {
                panic!(
                    "Artefato temporário encontrado no checkout ativo do crate: {:?}",
                    file
                );
            }
        }
    }
}

#[test]
fn test_cargo_manifest_consistency() {
    // Cargo.toml sanity: the package name and main binary path line up.
    // Cheap drift detector — catches accidental renames that would
    // break the flake input consumer
    // (`garos/flake.nix` -> `gar-cli.packages.x86_64-linux.default`).
    let manifest = fs::read_to_string(repo_root().join("Cargo.toml")).expect("read Cargo.toml");

    assert!(
        manifest.contains("name = \"gar\""),
        "Cargo.toml: package name must remain `gar` — the flake input \
         `gar-cli` in `garos/flake.nix` depends on it"
    );
    assert!(
        manifest.contains("path = \"src/main.rs\""),
        "Cargo.toml: main binary path must remain `src/main.rs`"
    );
}

#[test]
fn test_flake_nix_consistency() {
    // flake.nix sanity: must expose `packages.default`, must NOT
    // reference the legacy ragos monorepo by name in its OWN header
    // comment.
    let flake = fs::read_to_string(repo_root().join("flake.nix")).expect("read flake.nix");

    assert!(
        flake.contains("packages.default"),
        "flake.nix must expose `packages.default` — the `garos/flake.nix` \
         input consumes this exact output path"
    );
    assert!(
        flake.contains("GAR CLI"),
        "flake.nix description must say `GAR CLI` (not RAGOS) — the README \
         and K-128B cross-repo map already say `GAR`; drift here would \
         confuse downstream readers"
    );
    assert!(
        !flake.contains("RAGOS monorepo"),
        "flake.nix must not reference the legacy `RAGOS monorepo` — drift"
    );
}
