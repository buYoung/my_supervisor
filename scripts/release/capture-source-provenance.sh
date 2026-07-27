#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "$0")/../.." && pwd)"

fail() {
  printf 'source provenance failed: %s\n' "$1" >&2
  exit 1
}

usage() {
  printf 'usage: %s <create|verify> <workspace-relative target manifest path>\n' "$0" >&2
  exit 2
}

[[ $# -eq 2 ]] || usage
mode="$1"
manifest_argument="$2"

case "${manifest_argument}" in
  /*) manifest_path="${manifest_argument}" ;;
  *) manifest_path="${workspace_root}/${manifest_argument}" ;;
esac
case "${manifest_path}" in
  "${workspace_root}"/*) ;;
  *) fail "manifest path must be inside the workspace" ;;
esac

list_repository_paths() {
  git -C "${workspace_root}" ls-files --cached --others --exclude-standard -z \
    | perl -0 -e 'my @paths = sort split /\0/, do { local $/; <> }; print join "\0", @paths; print "\0" if @paths;'
}

is_build_input_path() {
  case "$1" in
    Cargo.toml|Cargo.lock|package.json|pnpm-lock.yaml|.cargo/*|scripts/release/*)
      return 0
      ;;
    crates/*.md|crates/*/AGENTS.md)
      return 1
      ;;
    crates/*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

list_build_inputs() {
  local relative_path
  while IFS= read -r -d '' relative_path; do
    if is_build_input_path "${relative_path}"; then
      printf '%s\0' "${relative_path}"
    fi
  done < <(list_repository_paths)
}

path_hex() {
  printf '%s' "$1" | od -An -tx1 | tr -d ' \n'
}

write_manifest() {
  local output_path="$1"
  local output_dir body_path head cargo_lock_digest entry_count=0
  output_dir="$(dirname "${output_path}")"
  mkdir -p "${output_dir}"
  body_path="$(mktemp "${output_dir}/.source-manifest.XXXXXX")"
  trap 'rm -f "${body_path}"' RETURN

  head="$(git -C "${workspace_root}" rev-parse HEAD)"
  cargo_lock_digest="$(shasum -a 256 "${workspace_root}/Cargo.lock" | awk '{print $1}')"
  {
    printf 'MSV_SOURCE_PROVENANCE_V1\n'
    printf 'head\t%s\n' "${head}"
    printf 'cargo_lock_sha256\t%s\n' "${cargo_lock_digest}"
    printf 'entries\tpath_hex\tmode_octal\tsha256\n'
    while IFS= read -r -d '' relative_path; do
      local input_path file_mode content_digest
      input_path="${workspace_root}/${relative_path}"
      [[ -f "${input_path}" ]] || fail "build input is not a regular file: ${relative_path}"
      file_mode="$(stat -f '%OLp' "${input_path}")"
      content_digest="$(shasum -a 256 "${input_path}" | awk '{print $1}')"
      printf 'entry\t%s\t%s\t%s\n' "$(path_hex "${relative_path}")" "${file_mode}" "${content_digest}"
      entry_count=$((entry_count + 1))
    done < <(list_build_inputs)
  } >"${body_path}"

  local manifest_digest
  manifest_digest="$(shasum -a 256 "${body_path}" | awk '{print $1}')"
  {
    cat "${body_path}"
    printf 'manifest_sha256\t%s\n' "${manifest_digest}"
  } >"${output_path}"
  rm -f "${body_path}"
  trap - RETURN
  printf 'source-provenance|manifest=%s|head=%s|cargo-lock-sha256=%s|entries=%s|sha256=%s\n' \
    "${manifest_path#"${workspace_root}/"}" "${head}" "${cargo_lock_digest}" "${entry_count}" "${manifest_digest}"
}

verify_manifest() {
  [[ -f "${manifest_path}" ]] || fail "manifest is missing at ${manifest_path}"
  local verification_path
  verification_path="$(mktemp "$(dirname "${manifest_path}")/.source-manifest-verify.XXXXXX")"
  trap 'rm -f "${verification_path}"' RETURN
  write_manifest "${verification_path}" >/dev/null
  cmp -s "${manifest_path}" "${verification_path}" || fail "source bytes, modes, tracked/untracked input set, HEAD, or Cargo.lock changed after provenance capture"
  rm -f "${verification_path}"
  trap - RETURN
  printf 'source-provenance-verified|manifest=%s|sha256=%s\n' \
    "${manifest_path#"${workspace_root}/"}" \
    "$(awk -F '\t' '$1 == "manifest_sha256" { print $2 }' "${manifest_path}")"
}

case "${mode}" in
  create) write_manifest "${manifest_path}" ;;
  verify) verify_manifest ;;
  *) usage ;;
esac
