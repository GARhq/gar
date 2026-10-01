#!/usr/bin/env bash
# sync-and-status.sh — fetch + status consolidado dos 6 repos GAROS
# Uso: ./scripts/sync-and-status.sh
# Saída: tabela com branch, ahead/behind, dirty, última tag

set -euo pipefail

ROOT="/home/garton/Projetos/garos-dev"
REPOS=(GAROS GAROSInstaller gar garos-control-api garos-control-web Artigo-latex-tamplat)

printf "%-22s %-10s %-10s %-8s %-10s %s\n" "REPO" "BRANCH" "REMOTE" "Δ" "DIRTY" "LAST TAG"
printf "%-22s %-10s %-10s %-8s %-10s %s\n" "----" "------" "------" "-" "-----" "--------"

for repo in "${REPOS[@]}"; do
  dir="$ROOT/$repo"
  [[ -d "$dir/.git" ]] || { printf "%-22s %s\n" "$repo" "(no git)"; continue; }

  branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "?")
  upstream=$(git -C "$dir" rev-parse --abbrev-ref --symbolic-full-name "@{u}" 2>/dev/null || echo "")

  if [[ -n "$upstream" ]]; then
    git -C "$dir" fetch origin --prune --quiet 2>&1 || true
    ab=$(git -C "$dir" rev-list --left-right --count "HEAD...$upstream" 2>/dev/null || echo "0 0")
    delta="$ab"
  else
    delta="(no up)"
  fi

  if [[ -n "$(git -C "$dir" status --porcelain 2>/dev/null)" ]]; then
    dirty="YES"
  else
    dirty="clean"
  fi

  tag=$(git -C "$dir" describe --tags --abbrev=0 2>/dev/null || echo "(no tag)")

  printf "%-22s %-10s %-10s %-8s %-10s %s\n" "$repo" "$branch" "$upstream" "$delta" "$dirty" "$tag"
done
