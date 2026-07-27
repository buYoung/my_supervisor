#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "$0")/../.." && pwd -P)"
host_triple="$(rustc -vV | sed -n 's/^host: //p')"
target_root="${CARGO_TARGET_DIR:-${workspace_root}/target}"
case "${target_root}" in
  /*) ;;
  *) target_root="${workspace_root}/${target_root}" ;;
esac
target_root="$(cd "${target_root}" && pwd -P)"
if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
  target_triple="${CARGO_BUILD_TARGET}"
  desktop_artifact_dir="${target_root}/${target_triple}/release"
else
  target_triple="${host_triple}"
  desktop_artifact_dir="${target_root}/release"
fi
if [[ "${target_triple}" == "${host_triple}" ]]; then
  sidecar_artifact_dir="${target_root}/release"
else
  sidecar_artifact_dir="${target_root}/${target_triple}/release"
fi
app_path="${1:-${desktop_artifact_dir}/bundle/macos/my-supervisor.app}"
dmg_path="${2:-}"
source_manifest_path="${3:-}"
expected_identifier="com.my-supervisor.desktop"
expected_version="0.1.0"
expected_binaries=(my-supervisor msv-daemon msv msv-log-proxy msv-group-reaper)
approved_csp="default-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' asset: data:; font-src 'self' data:; connect-src 'self'"
approved_entitlement_hex="3c3f786d6c2076657273696f6e3d22312e302220656e636f64696e673d225554462d38223f3e0a3c21444f435459504520706c697374205055424c494320222d2f2f4170706c652f2f44544420504c49535420312e302f2f454e222022687474703a2f2f7777772e6170706c652e636f6d2f445444732f50726f70657274794c6973742d312e302e647464223e0a3c706c6973742076657273696f6e3d22312e30223e0a3c646963742f3e0a3c2f706c6973743e0a"
approved_entitlement_sha256="97704a8960b4facceef54397a08fb5d0a456247c3627359215aa2a27df22656c"
approved_csp_hex="$(printf '%s' "${approved_csp}" | xxd -p -c 1000)"
dmg_mount_point=""
mounted_device=""
dmg_attached=0
canonical_dmg_mount_root=""

fail() {
  printf 'macOS package verification failed: %s\n' "$1" >&2
  exit 1
}

cleanup_mounted_dmg() {
  local cleanup_status=0 detach_target="" detached_target=""
  if [[ "${dmg_attached}" == '1' ]]; then
    if [[ -n "${mounted_device}" ]]; then
      detach_target="${mounted_device}"
    elif [[ -n "${dmg_mount_point}" ]]; then
      detach_target="${dmg_mount_point}"
    fi
    if [[ -n "${detach_target}" ]] && hdiutil detach "${detach_target}" >/dev/null 2>&1; then
      mounted_device=""
      dmg_attached=0
      detached_target="${detach_target}"
    else
      cleanup_status=1
    fi
  fi
  if [[ "${dmg_attached}" == '0' && -n "${dmg_mount_point}" ]]; then
    if rmdir "${dmg_mount_point}" >/dev/null 2>&1; then
      if [[ -n "${detached_target}" ]]; then
        printf 'cleanup|detach-target=%s|mount-directory=removed\n' "${detached_target}"
      fi
      dmg_mount_point=""
      canonical_dmg_mount_root=""
    else
      cleanup_status=1
    fi
  fi
  return "${cleanup_status}"
}

cleanup_on_exit() {
  local exit_status=$?
  trap - EXIT
  if ! cleanup_mounted_dmg; then
    printf 'macOS package verification failed: could not clean up the verifier-created DMG mount\n' >&2
    [[ "${exit_status}" == '0' ]] && exit_status=1
  fi
  exit "${exit_status}"
}

trap cleanup_on_exit EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

[[ -n "${source_manifest_path}" ]] || fail "source provenance manifest is required as the third argument"
[[ "${target_triple}" == 'aarch64-apple-darwin' ]] || fail "first-release package policy requires aarch64-apple-darwin"
case "${source_manifest_path}" in
  /*) ;;
  *) source_manifest_path="${workspace_root}/${source_manifest_path}" ;;
esac
[[ -f "${source_manifest_path}" ]] || fail "source provenance manifest is missing at ${source_manifest_path}"
"${workspace_root}/scripts/release/capture-source-provenance.sh" verify "${source_manifest_path}" >/dev/null
source_manifest_digest="$(awk -F '\t' '$1 == "manifest_sha256" { print $2 }' "${source_manifest_path}")"
[[ "${source_manifest_digest}" =~ ^[[:xdigit:]]{64}$ ]] || fail "source provenance manifest digest is invalid"

require_arm64_architecture() {
  [[ "$1" == "arm64" ]] || fail "expected arm64-only artifact, found $1"
}

find_mounted_device_for_mount_point() {
  local attach_plist="$1" requested_mount_point="$2" entity_index=0 entity_plist
  local entity_mount_point entity_device canonical_entity_mount_point
  while true; do
    if ! entity_plist="$(printf '%s' "${attach_plist}" \
      | plutil -extract "system-entities.${entity_index}" xml1 -o - - 2>/dev/null)"; then
      break
    fi
    entity_mount_point="$(printf '%s' "${entity_plist}" \
      | plutil -extract mount-point raw -o - - 2>/dev/null || true)"
    canonical_entity_mount_point="$(canonicalize_existing_path "${entity_mount_point}" 2>/dev/null || true)"
    if [[ "${canonical_entity_mount_point}" == "${requested_mount_point}" ]]; then
      entity_device="$(printf '%s' "${entity_plist}" \
        | plutil -extract dev-entry raw -o - - 2>/dev/null || true)"
      [[ "${entity_device}" =~ ^/dev/disk[0-9]+(s[0-9]+)?$ ]] || return 2
      printf '%s\n' "${entity_device}"
      return 0
    fi
    entity_index=$((entity_index + 1))
  done
  return 1
}

canonicalize_existing_path() {
  perl -MCwd=abs_path -e 'my $path = abs_path($ARGV[0]) // exit 1; print "$path\n";' "$1"
}

require_dmg_mount_containment() {
  local checked_path="$1" checked_label="$2" canonical_checked_path
  [[ -n "${canonical_dmg_mount_root}" ]] || return 0
  canonical_checked_path="$(canonicalize_existing_path "${checked_path}")" \
    || fail "could not canonicalize DMG ${checked_label}"
  [[ "${canonical_checked_path}" == "${canonical_dmg_mount_root}"/* ]] \
    || fail "DMG ${checked_label} escapes the verifier-created mount"
}

verify_security_contract() {
  local desktop_binary="$1" candidate_marker marker_found=0
  local expected_marker
  require_dmg_mount_containment "${desktop_binary}" 'desktop executable'
  expected_marker="MSV_SECURITY_CONTRACT_V1|csp_hex=${approved_csp_hex}|hardened_runtime=true|entitlement_path=entitlements.plist|entitlement_sha256=${approved_entitlement_sha256}|entitlement_hex=${approved_entitlement_hex}|source_provenance_sha256=${source_manifest_digest}"
  while IFS= read -r candidate_marker; do
    if [[ "${candidate_marker}" == "${expected_marker}" ]]; then
      marker_found=1
      break
    fi
  done < <(strings -a "${desktop_binary}")
  [[ "${marker_found}" == '1' ]] \
    || fail "desktop candidate does not contain the approved security/provenance contract marker"
}

verify_embedded_entitlements() {
  local desktop_binary="$1" entitlement_output entitlement_error signature_details
  require_dmg_mount_containment "${desktop_binary}" 'desktop executable'
  entitlement_output="$(mktemp "${target_root}/.embedded-entitlements.XXXXXX")"
  entitlement_error="$(mktemp "${target_root}/.embedded-entitlements-error.XXXXXX")"
  trap 'rm -f "${entitlement_output}" "${entitlement_error}"' RETURN
  if codesign -d --entitlements :- "${desktop_binary}" >"${entitlement_output}" 2>"${entitlement_error}"; then
    if [[ -s "${entitlement_output}" ]]; then
      plutil -lint "${entitlement_output}" >/dev/null \
        || fail "signed desktop candidate did not expose a valid embedded entitlement plist"
      [[ "$(plutil -convert json -o - "${entitlement_output}" | tr -d '[:space:]')" == '{}' ]] \
        || fail "signed desktop candidate contains an unexpected embedded entitlement"
      printf 'embedded-entitlements=signed-empty\n'
    else
      signature_details="$(codesign -dvv "${desktop_binary}" 2>&1)" \
        || fail "codesign could not identify the empty-entitlement candidate"
      grep -Fq 'Signature=adhoc' <<<"${signature_details}" \
        || fail "signed desktop candidate is missing required embedded entitlement evidence"
      printf 'embedded-entitlements=unsigned-empty-output\n'
    fi
  else
    if ! grep -Eq 'not signed at all|code object is not signed' "${entitlement_error}"; then
      fail "codesign could not inspect candidate entitlements"
    fi
    printf 'embedded-entitlements=unsigned-unavailable\n'
  fi
  rm -f "${entitlement_output}" "${entitlement_error}"
  trap - RETURN
}

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

scan_credential_inputs() {
  local credential_pattern matches=()
  credential_pattern='BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY|Authorization: Bearer [[:alnum:]_.-]+|APPLE_'
  credential_pattern+='(CERTIFICATE|API_KEY|ID_PASSWORD)='
  while IFS= read -r -d '' relative_path; do
    if LC_ALL=C grep -a -qE "${credential_pattern}" -- "${workspace_root}/${relative_path}"; then
      matches+=("${relative_path}")
    fi
  done < <(list_build_inputs)
  if ((${#matches[@]})); then
    printf 'credential-like material found in build inputs:\n' >&2
    printf '%s\n' "${matches[@]}" >&2
    exit 1
  fi
}

verify_bundle_identity_and_layout() {
  local bundle_path="$1" bundle_label="$2" bundle_info_plist bundle_macos_dir
  local bundle_identifier bundle_version bundle_executable actual_binary_count
  [[ -d "${bundle_path}" ]] || fail "${bundle_label} app bundle is missing at ${bundle_path}"
  if [[ "${bundle_label}" == 'DMG' ]]; then
    [[ ! -L "${bundle_path}" ]] || fail "DMG my-supervisor.app entry must not be a symlink"
    require_dmg_mount_containment "${bundle_path}" 'app bundle'
  fi
  bundle_info_plist="${bundle_path}/Contents/Info.plist"
  bundle_macos_dir="${bundle_path}/Contents/MacOS"
  [[ -f "${bundle_info_plist}" ]] || fail "${bundle_label} Info.plist is missing"
  [[ -d "${bundle_macos_dir}" ]] || fail "${bundle_label} Contents/MacOS is missing"
  [[ -d "${bundle_path}/Contents/Resources" ]] || fail "${bundle_label} Contents/Resources is missing"
  require_dmg_mount_containment "${bundle_info_plist}" 'Info.plist'
  require_dmg_mount_containment "${bundle_macos_dir}" 'Contents/MacOS'
  require_dmg_mount_containment "${bundle_path}/Contents/Resources" 'Contents/Resources'

  bundle_identifier="$(plutil -extract CFBundleIdentifier raw "${bundle_info_plist}")"
  bundle_version="$(plutil -extract CFBundleShortVersionString raw "${bundle_info_plist}")"
  bundle_executable="$(plutil -extract CFBundleExecutable raw "${bundle_info_plist}")"
  [[ "${bundle_identifier}" == "${expected_identifier}" ]] \
    || fail "unexpected ${bundle_label} bundle identifier ${bundle_identifier}"
  [[ "${bundle_version}" == "${expected_version}" ]] \
    || fail "unexpected ${bundle_label} bundle version ${bundle_version}"
  [[ "${bundle_executable}" == 'my-supervisor' ]] \
    || fail "unexpected ${bundle_label} bundle executable ${bundle_executable}"

  actual_binary_count="$(find "${bundle_macos_dir}" -maxdepth 1 -type f -perm -111 | wc -l | tr -d ' ')"
  [[ "${actual_binary_count}" == "${#expected_binaries[@]}" ]] \
    || fail "expected ${#expected_binaries[@]} ${bundle_label} executables, found ${actual_binary_count}"
}

verify_bundle_executables() {
  local bundle_path="$1" bundle_label="$2" peer_bundle_path="${3:-}"
  local bundle_macos_dir="${bundle_path}/Contents/MacOS" binary_name bundled_binary source_binary
  local peer_binary file_output mode architecture digest
  for binary_name in "${expected_binaries[@]}"; do
    bundled_binary="${bundle_macos_dir}/${binary_name}"
    [[ -f "${bundled_binary}" ]] || fail "missing ${bundle_label} executable ${binary_name}"
    [[ -x "${bundled_binary}" ]] || fail "${bundle_label} ${binary_name} is not executable"
    require_dmg_mount_containment "${bundled_binary}" "${binary_name} executable"
    file_output="$(file "${bundled_binary}")"
    [[ "${file_output}" == *"Mach-O 64-bit executable"* ]] \
      || fail "${bundle_label} ${binary_name} is not a 64-bit Mach-O executable"

    if [[ "${binary_name}" == 'my-supervisor' ]]; then
      source_binary="${desktop_artifact_dir}/${binary_name}"
    else
      source_binary="${sidecar_artifact_dir}/${binary_name}"
    fi
    [[ -f "${source_binary}" ]] || fail "release source artifact ${binary_name} is missing"
    cmp -s "${source_binary}" "${bundled_binary}" \
      || fail "${bundle_label} ${binary_name} differs from its release source artifact"

    if [[ -n "${peer_bundle_path}" ]]; then
      peer_binary="${peer_bundle_path}/Contents/MacOS/${binary_name}"
      cmp -s "${peer_binary}" "${bundled_binary}" \
        || fail "${bundle_label} ${binary_name} differs from the supplied app"
    fi

    mode="$(stat -f '%OLp' "${bundled_binary}")"
    architecture="$(lipo -archs "${bundled_binary}")"
    require_arm64_architecture "${architecture}"
    digest="$(shasum -a 256 "${bundled_binary}" | awk '{print $1}')"
    printf '%s|bundle=%s|mode=%s|arch=%s|sha256=%s\n' \
      "${binary_name}" "${bundle_label}" "${mode}" "${architecture}" "${digest}"
  done
}

verify_dmg_root_layout() {
  local root_entry root_entry_name root_entry_count=0
  local has_volume_icon=0 has_finder_layout=0 has_applications_link=0 has_app_bundle=0
  while IFS= read -r -d '' root_entry; do
    root_entry_name="${root_entry##*/}"
    root_entry_count=$((root_entry_count + 1))
    case "${root_entry_name}" in
      .VolumeIcon.icns)
        [[ -f "${root_entry}" && ! -L "${root_entry}" ]] || fail "DMG .VolumeIcon.icns is not a regular file"
        require_dmg_mount_containment "${root_entry}" '.VolumeIcon.icns'
        has_volume_icon=1
        ;;
      .DS_Store)
        [[ -f "${root_entry}" && ! -L "${root_entry}" && ! -x "${root_entry}" ]] \
          || fail "DMG .DS_Store is not a regular non-executable file"
        require_dmg_mount_containment "${root_entry}" '.DS_Store'
        has_finder_layout=1
        ;;
      Applications)
        [[ -L "${root_entry}" && "$(readlink "${root_entry}")" == '/Applications' ]] \
          || fail "DMG Applications entry is not the expected /Applications symlink"
        has_applications_link=1
        ;;
      my-supervisor.app)
        [[ -d "${root_entry}" && ! -L "${root_entry}" ]] || fail "DMG my-supervisor.app entry is not a real directory"
        require_dmg_mount_containment "${root_entry}" 'app bundle'
        has_app_bundle=1
        ;;
      *)
        fail "DMG contains an unexpected root entry ${root_entry_name}"
        ;;
    esac
  done < <(find "${dmg_mount_point}" -mindepth 1 -maxdepth 1 -print0)
  [[ "${root_entry_count}" == '4' && "${has_volume_icon}" == '1' && "${has_finder_layout}" == '1' \
    && "${has_applications_link}" == '1' && "${has_app_bundle}" == '1' ]] \
    || fail "DMG root layout does not contain exactly the expected app, Applications link, Finder layout, and volume icon"
}

verify_dmg() {
  local attach_plist dmg_digest mounted_app_path
  [[ -f "${dmg_path}" ]] || fail "DMG is missing at ${dmg_path}"
  hdiutil verify "${dmg_path}" >/dev/null || fail "DMG checksum verification failed"
  dmg_mount_point="$(mktemp -d "${target_root}/.package-dmg-mount.XXXXXX")"
  dmg_mount_point="$(canonicalize_existing_path "${dmg_mount_point}")" \
    || fail "could not canonicalize the verifier-created DMG mount point"
  if ! attach_plist="$(hdiutil attach -readonly -nobrowse -noverify -plist -mountpoint "${dmg_mount_point}" "${dmg_path}")"; then
    fail "DMG could not be mounted read-only"
  fi
  dmg_attached=1
  canonical_dmg_mount_root="$(canonicalize_existing_path "${dmg_mount_point}")" \
    || fail "could not canonicalize the mounted DMG root"
  if [[ "${MSV_PACKAGE_INJECT_ATTACH_PARSER_FAILURE:-0}" == '1' ]]; then
    fail "injected attach parser failure"
  fi
  if ! mounted_device="$(find_mounted_device_for_mount_point "${attach_plist}" "${dmg_mount_point}")"; then
    fail "DMG mount did not report the verifier-created mount point and device"
  fi

  verify_dmg_root_layout
  mounted_app_path="${dmg_mount_point}/my-supervisor.app"
  verify_bundle_identity_and_layout "${mounted_app_path}" 'DMG'
  verify_bundle_executables "${mounted_app_path}" 'DMG' "${app_path}"
  verify_security_contract "${mounted_app_path}/Contents/MacOS/my-supervisor"
  verify_embedded_entitlements "${mounted_app_path}/Contents/MacOS/my-supervisor"

  cleanup_mounted_dmg || fail "could not detach the verifier-mounted DMG"
  dmg_digest="$(shasum -a 256 "${dmg_path}" | awk '{print $1}')"
  printf 'dmg|sha256=%s|architecture=arm64|source-manifest-sha256=%s|internal-app=byte-equal\n' \
    "${dmg_digest}" "${source_manifest_digest}"
}

run_negative_self_checks() (
  local temporary_root copied_app canary_path marker_copy different_dmg_root different_dmg
  local tampered_dmg_root tampered_dmg symlink_dmg_root symlink_dmg parser_failure_log
  temporary_root="$(mktemp -d "${target_root}/.package-negative.XXXXXX")"
  canary_path="${workspace_root}/scripts/release/.package-security-credential-canary"
  trap 'rm -f "${canary_path}"; rm -rf "${temporary_root}"' EXIT

  copied_app="${temporary_root}/my-supervisor.app"
  cp -R "${app_path}" "${copied_app}"
  cp "${macos_dir}/msv" "${copied_app}/Contents/MacOS/my-supervisor"
  if (MSV_PACKAGE_NEGATIVE_SELF_CHECK=0 "$0" "${copied_app}" '' "${source_manifest_path}") >/dev/null 2>&1; then
    fail "negative self-check accepted a replaced desktop executable"
  fi

  marker_copy="${temporary_root}/marker-copy"
  cp "${macos_dir}/my-supervisor" "${marker_copy}"
  perl -0pi -e 's/MSV_SECURITY_CONTRACT_V1/MSV_SECURITY_CONTRACT_X1/' "${marker_copy}"
  if (verify_security_contract "${marker_copy}") 2>/dev/null; then
    fail "negative self-check accepted a corrupted desktop security marker"
  fi

  canary_name='APPLE_API_KEY'
  printf '%s=canary\n' "${canary_name}" >"${canary_path}"
  if (scan_credential_inputs) 2>/dev/null; then
    fail "negative self-check accepted an untracked credential canary"
  fi
  rm -f "${canary_path}"

  if (require_arm64_architecture 'x86_64') 2>/dev/null; then
    fail "negative self-check accepted x86_64"
  fi

  [[ -n "${dmg_path}" ]] || fail "negative DMG self-check requires a supplied DMG"
  different_dmg_root="${temporary_root}/different-dmg-root"
  mkdir "${different_dmg_root}"
  cp -R "${app_path}" "${different_dmg_root}/my-supervisor.app"
  ln -s /Applications "${different_dmg_root}/Applications"
  : >"${different_dmg_root}/.VolumeIcon.icns"
  : >"${different_dmg_root}/.DS_Store"
  printf 'different valid DMG\n' >"${different_dmg_root}/unexpected-root-entry"
  different_dmg="${temporary_root}/different-valid.dmg"
  hdiutil create -volname 'different-valid' -srcfolder "${different_dmg_root}" -fs HFS+ \
    -format UDZO -ov "${different_dmg}" >/dev/null
  if (MSV_PACKAGE_NEGATIVE_SELF_CHECK=0 "$0" "${app_path}" "${different_dmg}" "${source_manifest_path}") >/dev/null 2>&1; then
    fail "negative self-check accepted a checksum-valid DMG with an arbitrary root entry"
  fi

  tampered_dmg_root="${temporary_root}/tampered-dmg-root"
  mkdir "${tampered_dmg_root}"
  cp -R "${app_path}" "${tampered_dmg_root}/my-supervisor.app"
  ln -s /Applications "${tampered_dmg_root}/Applications"
  : >"${tampered_dmg_root}/.VolumeIcon.icns"
  : >"${tampered_dmg_root}/.DS_Store"
  cp "${macos_dir}/msv" "${tampered_dmg_root}/my-supervisor.app/Contents/MacOS/my-supervisor"
  cp "${macos_dir}/msv-group-reaper" "${tampered_dmg_root}/my-supervisor.app/Contents/MacOS/msv-log-proxy"
  tampered_dmg="${temporary_root}/tampered-main-and-sidecar.dmg"
  hdiutil create -volname 'my-supervisor' -srcfolder "${tampered_dmg_root}" -fs HFS+ \
    -format UDZO -ov "${tampered_dmg}" >/dev/null
  if (MSV_PACKAGE_NEGATIVE_SELF_CHECK=0 "$0" "${app_path}" "${tampered_dmg}" "${source_manifest_path}") >/dev/null 2>&1; then
    fail "negative self-check accepted a DMG with changed internal main and sidecar executables"
  fi

  symlink_dmg_root="${temporary_root}/symlink-app-dmg-root"
  mkdir "${symlink_dmg_root}"
  ln -s "${app_path}" "${symlink_dmg_root}/my-supervisor.app"
  ln -s /Applications "${symlink_dmg_root}/Applications"
  : >"${symlink_dmg_root}/.VolumeIcon.icns"
  : >"${symlink_dmg_root}/.DS_Store"
  symlink_dmg="${temporary_root}/symlink-app.dmg"
  hdiutil create -volname 'my-supervisor' -srcfolder "${symlink_dmg_root}" -fs HFS+ \
    -format UDZO -ov "${symlink_dmg}" >/dev/null
  if (MSV_PACKAGE_NEGATIVE_SELF_CHECK=0 "$0" "${app_path}" "${symlink_dmg}" "${source_manifest_path}") >/dev/null 2>&1; then
    fail "negative self-check accepted a my-supervisor.app symlink to an external app"
  fi

  parser_failure_log="${temporary_root}/attach-parser-failure.log"
  if MSV_PACKAGE_NEGATIVE_SELF_CHECK=0 MSV_PACKAGE_INJECT_ATTACH_PARSER_FAILURE=1 \
    "$0" "${app_path}" "${dmg_path}" "${source_manifest_path}" >"${parser_failure_log}" 2>&1; then
    fail "negative self-check accepted an injected attach parser failure"
  fi
  grep -Eq '^cleanup\|detach-target=.*/\.package-dmg-mount\.[[:alnum:]]+\|mount-directory=removed$' "${parser_failure_log}" \
    || fail "negative self-check did not observe exact mount-point detach and mount-directory removal after parser failure"
)

[[ -d "${app_path}" ]] || fail "app bundle is missing at ${app_path}"
info_plist="${app_path}/Contents/Info.plist"
macos_dir="${app_path}/Contents/MacOS"

verify_bundle_identity_and_layout "${app_path}" 'supplied'
verify_bundle_executables "${app_path}" 'supplied'
verify_security_contract "${macos_dir}/my-supervisor"
verify_embedded_entitlements "${macos_dir}/my-supervisor"
scan_credential_inputs

printf 'bundle|identifier=%s|version=%s|executables=%s|architecture=arm64|source-manifest-sha256=%s|credential-files=0\n' \
  "${expected_identifier}" "${expected_version}" "${#expected_binaries[@]}" "${source_manifest_digest}"

if [[ -n "${dmg_path}" ]]; then
  verify_dmg
fi

if [[ "${MSV_PACKAGE_NEGATIVE_SELF_CHECK:-0}" == '1' ]]; then
  run_negative_self_checks
  printf 'negative-self-checks=passed\n'
fi
