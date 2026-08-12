#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || -z "$1" ]]; then
  echo "usage: release-draft-ids.sh <expected-tag>" >&2
  exit 2
fi

expected_tag="$1"
release_pages="$(cat)"

if ! jq -e 'type == "array" and all(.[]; type == "array")' \
  <<< "$release_pages" >/dev/null; then
  echo "release listing must be a slurped array of API pages" >&2
  exit 1
fi

foreign_draft_count="$(jq -r --arg tag "$expected_tag" '
  [
    .[][] |
    select(
      .tag_name == $tag and
      .draft == true and
      .author.login != "github-actions[bot]"
    )
  ] |
  length
' <<< "$release_pages" | tr -d '\r')"
if [[ "$foreign_draft_count" != "0" ]]; then
  echo "refusing to delete a same-tag draft not created by github-actions[bot]" >&2
  exit 1
fi

jq -r --arg tag "$expected_tag" '
  .[][] |
  select(
    .tag_name == $tag and
    .draft == true and
    .author.login == "github-actions[bot]"
  ) |
  .id
' <<< "$release_pages" | tr -d '\r'
