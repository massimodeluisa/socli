#!/usr/bin/env bash
set -euo pipefail

: "${HOMEBREW_TAP:?set the HOMEBREW_TAP repository variable to owner/repo}"
: "${VERSION:?release version is required}"

if [[ -n "${HOMEBREW_TAP_SSH_KEY:-}" ]]; then
  tap_dir=$(mktemp -d)
  key_file=$(mktemp)
  hosts_file=$(mktemp)
  trap 'rm -f "$key_file" "$hosts_file"' EXIT
  chmod 600 "$key_file"
  printf '%s\n' "$HOMEBREW_TAP_SSH_KEY" > "$key_file"
  gh api meta --jq '.ssh_keys[] | "github.com " + .' > "$hosts_file"
  export GIT_SSH_COMMAND="ssh -i $key_file -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=$hosts_file"
  git clone --depth 1 "git@github.com:${HOMEBREW_TAP}.git" "$tap_dir"
  install -d "$tap_dir/Formula"
  install -m 644 artifacts/socli.rb "$tap_dir/Formula/socli.rb"
  git -C "$tap_dir" add Formula/socli.rb
  if ! git -C "$tap_dir" diff --cached --quiet; then
    git -C "$tap_dir" -c user.name=github-actions -c user.email=github-actions@users.noreply.github.com commit -m "chore: update socli to ${VERSION}"
    git -C "$tap_dir" push origin HEAD
  fi
  exit 0
fi

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
