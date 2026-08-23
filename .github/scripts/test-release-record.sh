#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
validator="$script_dir/validate-release-record.sh"
draft_selector="$script_dir/release-draft-ids.sh"
tag="v9.8.7"

valid_release='{
  "tag_name": "v9.8.7",
  "immutable": true,
  "draft": false,
  "assets": [
    {"name":"upstroke-x86_64-unknown-linux-gnu.tar.gz","state":"uploaded","digest":"sha256:0303030303030303030303030303030303030303030303030303030303030303"},
    {"name":"upstroke-aarch64-apple-darwin.tar.gz","state":"uploaded","digest":"sha256:0101010101010101010101010101010101010101010101010101010101010101"},
    {"name":"upstroke-x86_64-pc-windows-msvc.zip","state":"uploaded","digest":"sha256:0202020202020202020202020202020202020202020202020202020202020202"}
  ]
}'

expect_pass() {
  local name="$1"
  local body="$2"
  if ! printf '%s' "$body" | bash "$validator" "$tag"; then
    echo "expected release fixture to pass: $name" >&2
    exit 1
  fi
}

expect_fail() {
  local name="$1"
  local body="$2"
  if printf '%s' "$body" | bash "$validator" "$tag" 2>/dev/null; then
    echo "expected release fixture to fail: $name" >&2
    exit 1
  fi
}

expect_pass "complete immutable release" "$valid_release"
expect_fail "mutable release" "$(jq '.immutable = false' <<< "$valid_release")"
expect_fail "draft release" "$(jq '.draft = true' <<< "$valid_release")"
expect_fail "wrong tag" "$(jq '.tag_name = "v9.8.6"' <<< "$valid_release")"
expect_fail "missing asset" "$(jq 'del(.assets[0])' <<< "$valid_release")"
expect_fail "extra asset" "$(jq '.assets += [{"name":"extra.bin","state":"uploaded","digest":"sha256:0404040404040404040404040404040404040404040404040404040404040404"}]' <<< "$valid_release")"
expect_fail "asset without digest" "$(jq '.assets[0].digest = null' <<< "$valid_release")"
expect_fail "malformed digest" "$(jq '.assets[0].digest = "sha256:03"' <<< "$valid_release")"
expect_fail "asset still uploading" "$(jq '.assets[0].state = "new"' <<< "$valid_release")"

release_pages='[
  [
    {"id":101,"tag_name":"v9.8.7","draft":true,"author":{"login":"github-actions[bot]"}},
    {"id":102,"tag_name":"v9.8.7","draft":false,"author":{"login":"github-actions[bot]"}},
    {"id":201,"tag_name":"v1.0.0","draft":true,"author":{"login":"human-maintainer"}}
  ],
  [
    {"id":103,"tag_name":"v9.8.7","draft":true,"author":{"login":"github-actions[bot]"}}
  ]
]'
draft_ids="$(printf '%s' "$release_pages" | bash "$draft_selector" "$tag")"
if [[ "$draft_ids" != $'101\n103' ]]; then
  echo "draft selector did not return every and only same-tag draft" >&2
  exit 1
fi
if printf '%s' '{"not":"slurped pages"}' | bash "$draft_selector" "$tag" 2>/dev/null; then
  echo "draft selector accepted a malformed API response" >&2
  exit 1
fi
foreign_draft="$(jq '. += [[{"id":104,"tag_name":"v9.8.7","draft":true,"author":{"login":"human-maintainer"}}]]' <<< "$release_pages")"
if printf '%s' "$foreign_draft" | bash "$draft_selector" "$tag" 2>/dev/null; then
  echo "draft selector accepted a same-tag draft owned by another principal" >&2
  exit 1
fi

echo "release record fixtures: PASS"
