#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "$0")/../.." && pwd)"
target_root="${CARGO_TARGET_DIR:-${workspace_root}/target}"
if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
  target_triple="${CARGO_BUILD_TARGET}"
  artifact_dir="${target_root}/${target_triple}/release"
else
  target_triple="$(rustc -vV | sed -n 's/^host: //p')"
  artifact_dir="${target_root}/release"
fi
sidecar_dir="${workspace_root}/crates/desktop/binaries"
mkdir -p "${sidecar_dir}"

for binary_name in msv-daemon msv msv-log-proxy msv-group-reaper; do
  source_binary="${artifact_dir}/${binary_name}"
  sidecar_binary="${sidecar_dir}/${binary_name}-${target_triple}"
  if [[ ! -f "${source_binary}" || ! -x "${source_binary}" ]]; then
    printf 'sidecar preparation failed: release executable is missing at %s\n' "${source_binary}" >&2
    exit 1
  fi
  install -m 755 "${source_binary}" "${sidecar_binary}"
  cmp -s "${source_binary}" "${sidecar_binary}"
  printf 'prepared %s for %s\n' "${binary_name}" "${target_triple}"
done
