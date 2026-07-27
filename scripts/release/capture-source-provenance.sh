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

write_input_entries() {
  local format_version="$1" relative_path input_path tracked_state file_mode content_digest
  while IFS= read -r -d '' relative_path; do
    input_path="${workspace_root}/${relative_path}"
    [[ -f "${input_path}" ]] || fail "build input is not a regular file: ${relative_path}"
    file_mode="$(stat -f '%OLp' "${input_path}")"
    content_digest="$(shasum -a 256 "${input_path}" | awk '{print $1}')"
    if [[ "${format_version}" == 'V2' ]]; then
      if git -C "${workspace_root}" ls-files --cached --error-unmatch -- "${relative_path}" >/dev/null 2>&1; then
        tracked_state='tracked'
      else
        tracked_state='untracked'
      fi
      printf 'entry\t%s\t%s\t%s\t%s\n' \
        "$(path_hex "${relative_path}")" "${tracked_state}" "${file_mode}" "${content_digest}"
    else
      printf 'entry\t%s\t%s\t%s\n' "$(path_hex "${relative_path}")" "${file_mode}" "${content_digest}"
    fi
  done < <(list_build_inputs)
}

write_input_body() {
  local format_version="$1" output_path="$2" cargo_lock_digest
  cargo_lock_digest="$(shasum -a 256 "${workspace_root}/Cargo.lock" | awk '{print $1}')"
  {
    printf 'MSV_SOURCE_PROVENANCE_%s\n' "${format_version}"
    printf 'cargo_lock_sha256\t%s\n' "${cargo_lock_digest}"
    if [[ "${format_version}" == 'V2' ]]; then
      printf 'entries\tpath_hex\ttracked_state\tmode_octal\tsha256\n'
    else
      printf 'entries\tpath_hex\tmode_octal\tsha256\n'
    fi
    write_input_entries "${format_version}"
  } >"${output_path}"
}

extract_input_body() {
  local format_version="$1" source_path="$2" output_path="$3"
  awk -F '\t' -v expected_header="MSV_SOURCE_PROVENANCE_${format_version}" '
    NR == 1 {
      if ($0 != expected_header) exit 1
      print
      next
    }
    NR == 2 {
      if ($1 != "head" || NF != 2 || $2 !~ /^[[:xdigit:]]{40,64}$/) exit 1
      next
    }
    $1 == "manifest_sha256" {
      if (NF != 2 || $2 !~ /^[[:xdigit:]]{64}$/ || saw_footer) exit 1
      saw_footer = 1
      footer = $2
      next
    }
    {
      if (saw_footer) exit 1
      print
    }
    END {
      if (NR < 4 || !saw_footer) exit 1
    }
  ' "${source_path}" >"${output_path}"
}

manifest_field() {
  local field_name="$1" source_path="$2"
  awk -F '\t' -v field_name="${field_name}" '$1 == field_name { print $2 }' "${source_path}"
}

write_manifest() {
  local output_path="$1"
  local output_dir body_path head cargo_lock_digest entry_count manifest_digest
  output_dir="$(dirname "${output_path}")"
  mkdir -p "${output_dir}"
  body_path="$(mktemp "${output_dir}/.source-manifest-inputs.XXXXXX")"
  trap 'rm -f "${body_path}"' RETURN

  head="$(git -C "${workspace_root}" rev-parse HEAD)"
  write_input_body 'V2' "${body_path}"
  cargo_lock_digest="$(manifest_field 'cargo_lock_sha256' "${body_path}")"
  entry_count="$(awk -F '\t' '$1 == "entry" { count += 1 } END { print count + 0 }' "${body_path}")"

  manifest_digest="$(shasum -a 256 "${body_path}" | awk '{print $1}')"
  {
    printf 'MSV_SOURCE_PROVENANCE_V2\n'
    printf 'head\t%s\n' "${head}"
    sed -n '2,$p' "${body_path}"
    printf 'manifest_sha256\t%s\n' "${manifest_digest}"
  } >"${output_path}"
  rm -f "${body_path}"
  trap - RETURN
  printf 'source-provenance|manifest=%s|head=%s|cargo-lock-sha256=%s|entries=%s|sha256=%s\n' \
    "${manifest_path#"${workspace_root}/"}" "${head}" "${cargo_lock_digest}" "${entry_count}" "${manifest_digest}"
}

verify_manifest() {
  [[ -f "${manifest_path}" ]] || fail "manifest is missing at ${manifest_path}"
  local format_version expected_body_path current_body_path captured_digest computed_digest
  local captured_head current_head head_status
  format_version="$(sed -n '1p' "${manifest_path}")"
  case "${format_version}" in
    MSV_SOURCE_PROVENANCE_V2) format_version='V2' ;;
    MSV_SOURCE_PROVENANCE_V1) format_version='V1' ;;
    *) fail "unsupported source provenance manifest format" ;;
  esac
  expected_body_path="$(mktemp "$(dirname "${manifest_path}")/.source-manifest-expected.XXXXXX")"
  current_body_path="$(mktemp "$(dirname "${manifest_path}")/.source-manifest-current.XXXXXX")"
  trap 'rm -f "${expected_body_path}" "${current_body_path}"' RETURN
  extract_input_body "${format_version}" "${manifest_path}" "${expected_body_path}" \
    || fail "manifest structure is invalid"
  captured_digest="$(manifest_field 'manifest_sha256' "${manifest_path}")"
  [[ "${captured_digest}" =~ ^[[:xdigit:]]{64}$ ]] || fail "manifest digest is invalid"
  if [[ "${format_version}" == 'V2' ]]; then
    computed_digest="$(shasum -a 256 "${expected_body_path}" | awk '{print $1}')"
    [[ "${captured_digest}" == "${computed_digest}" ]] \
      || fail "manifest build-input digest does not match its recorded input rows"
  else
    computed_digest="$(sed '$d' "${manifest_path}" | shasum -a 256 | awk '{print $1}')"
    [[ "${captured_digest}" == "${computed_digest}" ]] \
      || fail "legacy manifest digest does not match its captured manifest body"
  fi
  write_input_body "${format_version}" "${current_body_path}"
  captured_head="$(manifest_field 'head' "${manifest_path}")"
  current_head="$(git -C "${workspace_root}" rev-parse HEAD)"
  if [[ "${captured_head}" == "${current_head}" ]]; then
    head_status='unchanged'
  else
    head_status='metadata-changed'
    printf 'source-provenance-note|manifest=%s|head-status=%s|head-captured=%s|head-current=%s\n' \
      "${manifest_path#"${workspace_root}/"}" "${head_status}" "${captured_head}" "${current_head}" >&2
  fi
  cmp -s "${expected_body_path}" "${current_body_path}" \
    || fail "build inputs changed after provenance capture (file bytes, modes, tracked state, input set, or Cargo.lock); HEAD is audit metadata only"
  rm -f "${expected_body_path}" "${current_body_path}"
  trap - RETURN
  printf 'source-provenance-verified|manifest=%s|sha256=%s|head-status=%s\n' \
    "${manifest_path#"${workspace_root}/"}" \
    "${captured_digest}" "${head_status}"
}

case "${mode}" in
  create) write_manifest "${manifest_path}" ;;
  verify) verify_manifest ;;
  *) usage ;;
esac
