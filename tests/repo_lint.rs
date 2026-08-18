use std::fs;
use std::path::{Path, PathBuf};

// Helper para localizar a raiz do monorepo garos
fn find_garos_repo_root() -> PathBuf {
    // A partir da pasta gar/tests, o repositório garos geralmente é o vizinho ../garos
    let current_dir = std::env::current_dir().expect("Failed to get current dir");
    
    // Testa caminhos comuns no workspace do desenvolvedor
    let candidates = vec![
        current_dir.join("../garos"),
        current_dir.join(".."), // se rodando no workspace root se houver
        PathBuf::from("/home/rocha/Proyectos/garos-dev/garos"),
        PathBuf::from("/home/ubuntu/Proyectos/garos-dev/garos"), // no container
    ];

    for candidate in candidates {
        if candidate.join("server").exists() && candidate.join("client").exists() {
            return candidate.canonicalize().unwrap_or(candidate);
        }
    }

    panic!(
        "Não foi possível encontrar a raiz do repositório 'garos'. \
         Diretório atual: {:?}",
        current_dir
    );
}

// Retorna uma lista de caminhos relativos ao diretório base
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
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();

                // Pula pastas do git e target
                if file_name == ".git" || file_name == "target" || file_name == "node_modules" || file_name == ".direnv" {
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
    let root = find_garos_repo_root();
    let allowed_mds = vec![
        "README.md",
        "CHANGELOG.md",
        "CONTRIBUTING.md",
        "INSTRUCOES.md",
        "INSTRUCT.md",
        "AGENTS.md",
        "CLAUDE.md",
        "CLI_CHEAT_SHEET.md",
        "IDEA.md", // Nova blueprint do GAROS
    ];

    let files = list_files_in_dir(&root, Some(1), false);
    for file in files {
        if let Some(ext) = file.extension() {
            if ext == "md" {
                let filename = file.file_name().unwrap().to_string_lossy();
                assert!(
                    allowed_mds.contains(&filename.as_ref()),
                    "Markdown no topo do repositório fora da allowlist: {}",
                    filename
                );
            }
        }
    }
}

#[test]
fn test_root_no_legacy_paths() {
    let root = find_garos_repo_root();
    
    // Pastas antigas renegadas
    let legacy_paths = vec!["garos", "SRV-GAROS"];
    for path in legacy_paths {
        let full_path = root.join(path);
        assert!(
            !full_path.exists(),
            "Árvore legada reintroduzida na raiz de garos: {}",
            path
        );
    }

    // DepartureMono font no root (deve ficar em themes/)
    let files = list_files_in_dir(&root, Some(1), false);
    for file in files {
        let filename = file.file_name().unwrap().to_string_lossy();
        assert!(
            !filename.starts_with("DepartureMono-"),
            "Asset vendorizado fora do domínio canônico themes/: {}",
            filename
        );
    }
}

#[test]
fn test_docs_layout() {
    let root = find_garos_repo_root();
    let docs_dir = root.join("docs");

    if !docs_dir.exists() {
        return; // Pula se docs não estiver no checkout (ex: CI minimal)
    }

    // Não deve conter subpastas em docs/ exceto archive/
    if let Ok(entries) = fs::read_dir(&docs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap().to_string_lossy();
                assert_eq!(
                    name, "archive",
                    "Subdiretório inesperado em docs/: {}",
                    name
                );
            }
        }
    }
}

#[test]
fn test_docs_headers_and_status() {
    let root = find_garos_repo_root();
    let docs_dir = root.join("docs");

    if !docs_dir.exists() {
        return;
    }

    let files = list_files_in_dir(&docs_dir, Some(1), false);
    for file in files {
        if file.extension().unwrap_or_default() != "md" || file.file_name().unwrap().to_string_lossy() == "README.md" {
            continue;
        }

        let content = fs::read_to_string(&file).expect("Failed to read doc file");
        
        // Verifica headers obrigatórios
        assert!(
            content.contains("Status: "),
            "Doc sem cabeçalho 'Status: ': {:?}",
            file.file_name().unwrap()
        );
        assert!(
            content.contains("Scope: "),
            "Doc sem cabeçalho 'Scope: ': {:?}",
            file.file_name().unwrap()
        );

        // Extrai status e valida
        let status_line = content.lines().find(|l| l.starts_with("Status: ")).unwrap();
        let status = status_line.replace("Status: ", "").trim().to_string();
        assert!(
            status == "canonical" || status == "secondary",
            "Doc {:?} possui status inválido: {}",
            file.file_name().unwrap(),
            status
        );

        // Docs canônicas exigem Last reviewed
        if status == "canonical" {
            let has_reviewed = content.lines().any(|l| {
                l.starts_with("Last reviewed: ") && l.len() >= 25 // formato YYYY-MM-DD
            });
            assert!(
                has_reviewed,
                "Doc canonical sem 'Last reviewed: YYYY-MM-DD' válido: {:?}",
                file.file_name().unwrap()
            );
        }
    }

    // Validar docs no arquivo (archive/)
    let archive_dir = docs_dir.join("archive");
    if archive_dir.exists() {
        let archive_files = list_files_in_dir(&archive_dir, Some(1), false);
        for file in archive_files {
            if file.extension().unwrap_or_default() != "md" {
                continue;
            }

            let content = fs::read_to_string(&file).expect("Failed to read archive doc");
            let status_line = content.lines().find(|l| l.starts_with("Status: "));
            assert!(
                status_line.is_some(),
                "Doc de archive sem 'Status: ': {:?}",
                file.file_name().unwrap()
            );

            let status = status_line.unwrap().replace("Status: ", "").trim().to_string();
            assert_eq!(
                status, "archived",
                "Doc em archive sem Status 'archived': {:?}",
                file.file_name().unwrap()
            );
        }
    }
}

#[test]
fn test_no_archive_references_in_active_docs() {
    let root = find_garos_repo_root();
    let docs_dir = root.join("docs");

    if !docs_dir.exists() {
        return;
    }

    let files = list_files_in_dir(&docs_dir, Some(1), false);
    for file in files {
        if file.extension().unwrap_or_default() != "md" || file.file_name().unwrap().to_string_lossy() == "README.md" {
            continue;
        }

        let content = fs::read_to_string(&file).expect("Failed to read file");
        assert!(
            !content.contains("docs/archive/") && !content.contains("archive/"),
            "Doc ativo {:?} referencia a pasta archive/",
            file.file_name().unwrap()
        );
    }
}

#[test]
fn test_scripts_layout() {
    let root = find_garos_repo_root();
    let scripts_dir = root.join("scripts");

    if !scripts_dir.exists() {
        return;
    }

    // Nenhum script solto em scripts/ (exceto README.md)
    if let Ok(entries) = fs::read_dir(&scripts_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().unwrap().to_string_lossy();
                assert_eq!(
                    name, "README.md",
                    "Script solto no diretório scripts/: {}",
                    name
                );
            } else if path.is_dir() {
                // Subdiretórios válidos de scripts
                let name = path.file_name().unwrap().to_string_lossy();
                let allowed_subdirs = vec!["dev", "ops", "tests", "lab"];
                assert!(
                    allowed_subdirs.contains(&name.as_ref()),
                    "Subdiretório inesperado em scripts/: {}",
                    name
                );
            }
        }
    }
}

#[test]
fn test_script_headers() {
    let root = find_garos_repo_root();
    let scripts_dir = root.join("scripts");

    if !scripts_dir.exists() {
        return;
    }

    let allowed_categories = vec!["dev", "ops", "tests", "lab"];
    for cat in allowed_categories {
        let cat_dir = scripts_dir.join(cat);
        if !cat_dir.exists() {
            continue;
        }

        let files = list_files_in_dir(&cat_dir, None, true);
        for file in files {
            // Pular diretórios e ler arquivos de script shell
            if file.extension().map(|e| e == "sh" || e == "py").unwrap_or(false) || file.file_name().unwrap().to_string_lossy().starts_with("test-") {
                let content = fs::read_to_string(&file).expect("Failed to read script");
                
                assert!(
                    content.contains("# Purpose: "),
                    "Script sem '# Purpose: ': {:?}",
                    file.file_name().unwrap()
                );
                
                let parent_dir_name = file.parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                let expected_cat_header = format!("# Category: {}", parent_dir_name);
                assert!(
                    content.contains(&expected_cat_header),
                    "Script com '# Category' divergente do diretório ({:?} vs {:?}): {:?}",
                    expected_cat_header,
                    parent_dir_name,
                    file.file_name().unwrap()
                );

                let has_safety = content.contains("# Safety: safe")
                    || content.contains("# Safety: destructive")
                    || content.contains("# Safety: lab-only");
                assert!(
                    has_safety,
                    "Script sem '# Safety' válido: {:?}",
                    file.file_name().unwrap()
                );
            }
        }
    }
}

#[test]
fn test_banned_references() {
    let root = find_garos_repo_root();
    
    // Padrões que não devem constar nos docs ativos do repositório
    let banned_patterns = vec![
        "docs/clients-inventory.csv",
        "flake/client.nix",
        "flake/server.nix",
        "flake/installer.nix",
        "server/server.nix",
        "installer/installer.nix",
        "garos/pxe/",
        "garos/scripts/provision-tftp.sh",
        "./scripts/migrate-garos-inventory.sh",
        "./scripts/test-clients-inventory-validation.sh",
        "./scripts/test-clients-inventory-routing.sh",
    ];

    let mut files = vec![
        root.join("README.md"),
        root.join("CONTRIBUTING.md"),
        root.join("scripts/README.md"),
    ];

    let docs_dir = root.join("docs");
    if docs_dir.exists() {
        files.extend(list_files_in_dir(&docs_dir, Some(1), false));
    }

    for file in files {
        if !file.exists() || file.is_dir() {
            continue;
        }

        let content = fs::read_to_string(&file).expect("Failed to read file");
        for pattern in &banned_patterns {
            assert!(
                !content.contains(pattern),
                "Referência antiga ou banida encontrada em {:?}: '{}'",
                file.file_name().unwrap(),
                pattern
            );
        }
    }
}

#[test]
fn test_inventory_rules() {
    let root = find_garos_repo_root();

    // 1. Não deve haver arquivos de inventário CSV no repositório ativo
    let files = list_files_in_dir(&root, None, true);
    for file in files {
        let name = file.file_name().unwrap().to_string_lossy();
        if name == "clients-inventory.csv" || name == "clients.csv" {
            // Pula docs/archive
            if !file.to_string_lossy().contains("docs/archive") {
                panic!("CSV de inventário reintroduzido no repositório ativo: {:?}", file);
            }
        }
    }

    // 2. Não deve referenciar clients-inventory.bootstrap.nix como fonte primária
    let check_dirs = vec![root.join("server"), root.join("flake")];
    for dir in check_dirs {
        if !dir.exists() {
            continue;
        }
        let nix_files = list_files_in_dir(&dir, None, true);
        for file in nix_files {
            if file.extension().unwrap_or_default() == "nix" {
                let content = fs::read_to_string(&file).expect("Failed to read nix file");
                for line in content.lines() {
                    let trim_line = line.trim();
                    // Valida se há importações diretas do bootstrap como fonte primária
                    if (trim_line.contains("import") || trim_line.contains("config =") || trim_line.contains("config +="))
                        && trim_line.contains("clients-inventory.bootstrap.nix")
                    {
                        panic!(
                            "servidor ou flake referenciando o inventory bootstrap como fonte primária: {:?}",
                            file.file_name().unwrap()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_no_temporary_artifacts() {
    let root = find_garos_repo_root();
    let files = list_files_in_dir(&root, None, true);
    
    for file in files {
        let name = file.file_name().unwrap().to_string_lossy();
        if name == "result" || name.starts_with("result-") || name.ends_with(".log") || name.ends_with(".tmp") || name.ends_with(".bak") || name == "nohup.out" {
            // Pular logs gerados pelo hermes de forma conhecida (dentro de .gemini/ ou logs do agent)
            if !file.to_string_lossy().contains(".gemini") && !file.to_string_lossy().contains(".git") {
                panic!("Artefato temporário encontrado no checkout ativo do repositório: {:?}", file);
            }
        }
    }
}
