#!/usr/bin/env bash
set -euo pipefail

: "${HOMEBREW_TAP:?set the HOMEBREW_TAP repository variable to owner/repo}"
: "${VERSION:?release version is required}"

branch=$(gh api "repos/${HOMEBREW_TAP}" --jq .default_branch)
endpoint="repos/${HOMEBREW_TAP}/contents/Formula/socli.rb"
files=$(gh api "repos/${HOMEBREW_TAP}/git/trees/${branch}" --jq '[.tree[].path]')
sha=''
if jq -e 'index("Formula") != null' <<< "$files" > /dev/null; then
  formula_files=$(gh api "repos/${HOMEBREW_TAP}/contents/Formula?ref=${branch}" --jq '[.[].name]')
  if jq -e 'index("socli.rb") != null' <<< "$formula_files" > /dev/null; then
    sha=$(gh api "${endpoint}?ref=${branch}" --jq .sha)
  fi
fi

payload=$(jq -n \
  --arg message "chore: update socli to ${VERSION}" \
  --arg content "$(base64 < artifacts/socli.rb | tr -d '\n')" \
  --arg branch "$branch" \
  --arg sha "$sha" \
  '{message: $message, content: $content, branch: $branch} + (if $sha == "" then {} else {sha: $sha} end)')
gh api --method PUT "$endpoint" --input - <<< "$payload" > /dev/null
