#!/usr/bin/env bash
# release.sh — versionamento SemVer + tag + changelog por repo GAROS
# Uso: ./scripts/release.sh <repo> <patch|minor|major> "<msg da tag>"
# Ex:  ./scripts/release.sh gar patch "fix: corrige path do flake"

set -euo pipefail

REPO="${1:?repo obrigatório: GAROS|GAROSInstaller|gar|garos-control-api|garos-control-web}"
BUMP="${2:?bump obrigatório: patch|minor|major}"
TAG_MSG="${3:?mensagem da tag obrigatória}"

ROOT="/home/garton/Projetos/garos-dev"
cd "$ROOT/$REPO"

# --- 1. Confirma remote e branch limpo ---
CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
echo "[$REPO] branch: $CURRENT_BRANCH"

if [[ -n "$(git status --porcelain)" ]]; then
  echo "ERRO: working tree sujo. Commit/stash antes de release." >&2
  git status -s
  exit 1
fi

# --- 2. Fetch + fast-fail se local divergiu ---
git fetch origin --tags --prune
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git rev-parse "origin/$CURRENT_BRANCH")
if [[ "$LOCAL" != "$REMOTE" ]]; then
  echo "ERRO: HEAD ($LOCAL) != origin/$CURRENT_BRANCH ($REMOTE)." >&2
  echo "Faça rebase ou push antes de taggear." >&2
  exit 1
fi

# --- 3. Calcula próxima versão SemVer ---
CURRENT_TAG=$(git tag --sort=-version:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -1 || echo "v0.0.0")
echo "[$REPO] tag atual: $CURRENT_TAG"

VER=${CURRENT_TAG#v}
IFS='.' read -r MAJOR MINOR PATCH <<< "$VER"

case "$BUMP" in
  patch) PATCH=$((PATCH + 1)) ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  *) echo "ERRO: bump deve ser patch|minor|major" >&2; exit 1 ;;
esac

NEW_TAG="v${MAJOR}.${MINOR}.${PATCH}"
echo "[$REPO] nova tag: $NEW_TAG  (${BUMP})"

# --- 4. Atualiza CHANGELOG.md (insere seção) ---
CHANGELOG="CHANGELOG.md"
DATE=$(date -u +%Y-%m-%d)

if [[ -f "$CHANGELOG" ]]; then
  TMP=$(mktemp)
  {
    echo "## [$NEW_TAG] - $DATE"
    echo ""
    echo "- $TAG_MSG"
    echo ""
    cat "$CHANGELOG"
  } > "$TMP"
  mv "$TMP" "$CHANGELOG"
else
  cat > "$CHANGELOG" <<EOF
# Changelog

## [$NEW_TAG] - $DATE

- $TAG_MSG
EOF
fi

# --- 5. Commit + tag ---
git add "$CHANGELOG"
git commit -m "release($REPO): $NEW_TAG"
git tag -a "$NEW_TAG" -m "$TAG_MSG"

# --- 6. Push commit + tag ---
git push origin "$CURRENT_BRANCH"
git push origin "$NEW_TAG"

echo ""
echo "✅ [$REPO] released $NEW_TAG"
echo "   commit: $(git rev-parse --short HEAD)"
echo "   tag:    $NEW_TAG (pushed)"
