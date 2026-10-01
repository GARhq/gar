# Política de Versionamento — GAROS Ecosystem

> **Regra de ouro:** versionamento é por-repo. Cada um dos 6 repos GAROS mantém
> sua própria trilha SemVer independente.

## SemVer aplicado

Seguimos [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.0.html).

| Bump   | Tag          | Quando usar                                              |
|--------|--------------|----------------------------------------------------------|
| patch  | `vX.Y.Z+1`   | Bug fix, refactor interno, docs, chore                  |
| minor  | `vX.Y+1.0`   | Nova feature compatível, novo módulo, novo endpoint     |
| major  | `vX+1.0.0`   | Breaking change, migration obrigatória, security fix crítica |

### Security fix crítica

Mesmo em `<1.0.0` (0.x.y), uma CVE/segurança crítica que afete produção **promove major**:

```
0.3.7 (vulnerável) → 1.0.0 (fix crítico, contrato endurecido)
```

Justificativa: força todo consumidor a olhar a nota de release.

## Convenção de tag

- Prefixo `v` (semver padrão): `v1.2.3`
- Tag anotada (`git tag -a`) com mensagem descrevendo o impacto
- Changelog por tag, gerado automaticamente via `scripts/release.sh`

## Workflow release

```
1. trabalho em feature/fix branch
2. PR merge → main
3. fetch + rebase local
4. ./scripts/release.sh <repo> <patch|minor|major> "<msg>"
   └─ valida working tree limpo
   └─ valida local == remote
   └─ bump SemVer
   └─ atualiza CHANGELOG.md
   └─ commit + tag
   └─ push branch + tag
```

## Conventional Commits

Todos os commits seguem [Conventional Commits](https://www.conventionalcommits.org/):

| Tipo       | Para                                                |
|------------|-----------------------------------------------------|
| `feat`     | Nova feature                                        |
| `fix`      | Bug fix                                             |
| `refactor` | Mudança interna sem mudar comportamento             |
| `perf`     | Ganho de performance mensurável                     |
| `docs`     | Só documentação                                     |
| `test`     | Só testes                                           |
| `chore`    | Build, deps, tooling                                |
| `build`    | Sistema de build / dependências externas            |
| `ci`       | CI/CD pipeline                                      |
| `style`    | Formatação (sem mudar lógica)                       |
| `revert`   | Reverte commit anterior                             |

Breaking change: sufixo `!` + footer `BREAKING CHANGE: ...`.

## Repos versionados

| Repo                  | Tag atual | Próxima planejada |
|-----------------------|-----------|-------------------|
| `GAROS`               | v1.1.0    | v1.1.1 patch      |
| `GAROSInstaller`      | (sem tag) | v0.1.0 minor      |
| `gar`                 | (sem tag) | v0.1.0 minor      |
| `garos-control-api`   | (sem tag) | v0.1.0 minor      |
| `garos-control-web`   | (sem tag) | v0.1.0 minor      |

## Scripts

| Script                        | Função                                 |
|-------------------------------|----------------------------------------|
| `scripts/release.sh`          | Bump + tag + changelog + push          |
| `scripts/sync-and-status.sh`  | Fetch + status consolidado dos 6 repos |

## Anti-padrões

- ❌ Tag local sem push
- ❌ Versionar com path local (`path:/...`) em CI/produção
- ❌ `v1` (sem patch) — sempre 3 segmentos
- ❌ Editar tag depois de pushed — bump novo
- ❌ Bump major pra "mostrar maturidade" sem breaking change
