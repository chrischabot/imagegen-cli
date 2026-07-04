#!/usr/bin/env bash
# =============================================================================
# imagegen — release script.
#
#   1. sanity checks          → clean tree, tests pass, version not yet tagged
#   2. git tag + push         → v<version> from Cargo.toml
#   3. gh release create      → GitHub release (source tarball is the artifact)
#   4. brew tap update        → bumps version + sha256 in
#                               chrischabot/homebrew-imagegen/Formula/imagegen.rb
#
# After this script exits cleanly, end users can install via:
#   brew tap chrischabot/imagegen
#   brew install imagegen
#
# Prerequisites: `gh` authenticated with repo scope on imagegen-cli and
# homebrew-imagegen (create the tap repo once: gh repo create
# chrischabot/homebrew-imagegen --public).
#
# Usage: ./release.sh
# =============================================================================
set -euo pipefail

GITHUB_REPO="chrischabot/imagegen-cli"
TAP_REPO="chrischabot/homebrew-imagegen"
FORMULA_PATH_IN_TAP="Formula/imagegen.rb"

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RED=$'\033[0;31m'; GREEN=$'\033[0;32m'; YELLOW=$'\033[1;33m'; NC=$'\033[0m'
step()  { echo "${GREEN}==>${NC} $*"; }
warn()  { echo "${YELLOW}warn:${NC} $*" >&2; }
fail()  { echo "${RED}error:${NC} $*" >&2; exit 1; }

# ---------- preflight ------------------------------------------------------
step "Preflight checks"
command -v cargo >/dev/null || fail "cargo not found"
command -v gh    >/dev/null || fail "gh (GitHub CLI) not found"
gh auth status   >/dev/null 2>&1 || fail "gh not authenticated — run: gh auth login"

cd "${PROJECT_DIR}"
[[ -z "$(git status --porcelain)" ]] || fail "working tree not clean — commit or stash first"

VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/version = "([^"]+)"/\1/')"
TAG="v${VERSION}"
[[ -n "${VERSION}" ]] || fail "could not read version from Cargo.toml"

if git rev-parse "${TAG}" >/dev/null 2>&1; then
  fail "tag ${TAG} already exists — bump the version in Cargo.toml first"
fi

step "Running tests"
cargo test --quiet
cargo clippy --all-targets --quiet -- -D warnings

# ---------- tag + release --------------------------------------------------
step "Tagging ${TAG} and pushing"
git tag -a "${TAG}" -m "imagegen ${VERSION}"
git push origin main "${TAG}"

step "Creating GitHub release ${TAG}"
gh release create "${TAG}" \
  --repo "${GITHUB_REPO}" \
  --title "imagegen ${VERSION}" \
  --notes "$(cat <<NOTES
imagegen ${VERSION} — fast, agent-friendly CLI for OpenAI image generation.

## Install via Homebrew
\`\`\`sh
brew tap chrischabot/imagegen
brew install imagegen
\`\`\`

## Or with cargo
\`\`\`sh
cargo install --git https://github.com/${GITHUB_REPO} --tag ${TAG}
\`\`\`
NOTES
)"

# ---------- sha256 of the source tarball -----------------------------------
step "Fetching release tarball for sha256"
TARBALL_URL="https://github.com/${GITHUB_REPO}/archive/refs/tags/${TAG}.tar.gz"
SHA256="$(curl -fsSL "${TARBALL_URL}" | shasum -a 256 | awk '{print $1}')"
[[ -n "${SHA256}" ]] || fail "failed to compute sha256 of ${TARBALL_URL}"

# ---------- publish: Homebrew tap ------------------------------------------
step "Updating Homebrew formula in ${TAP_REPO}"
TAP_CHECKOUT="$(mktemp -d)/homebrew-imagegen"
trap 'rm -rf "${TAP_CHECKOUT%/*}"' EXIT
gh repo clone "${TAP_REPO}" "${TAP_CHECKOUT}" -- --quiet

FORMULA_FILE="${TAP_CHECKOUT}/${FORMULA_PATH_IN_TAP}"
mkdir -p "$(dirname "${FORMULA_FILE}")"
# The template is the source of truth; version + sha are stamped in each time.
cp "${PROJECT_DIR}/homebrew/imagegen.rb" "${FORMULA_FILE}"
sed -i '' -E \
  -e "s|/tags/v[0-9][^\"]*\.tar\.gz|/tags/${TAG}.tar.gz|" \
  -e "s|^([[:space:]]*sha256 )\"[^\"]*\"|\\1\"${SHA256}\"|" \
  "${FORMULA_FILE}"

grep -q "tags/${TAG}.tar.gz" "${FORMULA_FILE}" \
  || fail "formula substitution failed: url not updated in ${FORMULA_FILE}"
grep -q "sha256 \"${SHA256}\"" "${FORMULA_FILE}" \
  || fail "formula substitution failed: sha256 not updated in ${FORMULA_FILE}"

if git -C "${TAP_CHECKOUT}" diff --quiet -- "${FORMULA_PATH_IN_TAP}" \
   && [[ -z "$(git -C "${TAP_CHECKOUT}" status --porcelain)" ]]; then
  warn "formula already at ${VERSION} — nothing to push"
else
  git -C "${TAP_CHECKOUT}" add "${FORMULA_PATH_IN_TAP}"
  git -C "${TAP_CHECKOUT}" commit -m "imagegen ${VERSION}" >/dev/null
  git -C "${TAP_CHECKOUT}" push origin HEAD >/dev/null 2>&1
  step "Formula updated to ${VERSION}"
fi

cat <<EOF

${GREEN}==> Released imagegen ${VERSION}${NC}

  Release: https://github.com/${GITHUB_REPO}/releases/tag/${TAG}
  SHA256:  ${SHA256}
  Tap:     https://github.com/${TAP_REPO}

Users can now install with:
  brew tap chrischabot/imagegen
  brew install imagegen
EOF
