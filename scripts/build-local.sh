#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "$0")/.." && pwd -P)"
build_target="${1:-}"
build_profile="${2:-debug}"
target_root="${CARGO_TARGET_DIR:-${workspace_root}/target}"
case "${target_root}" in
  /*) ;;
  *) target_root="${workspace_root}/${target_root}" ;;
esac

usage() {
  printf 'usage: %s <cli|app> [debug|release]\n' "$0" >&2
  exit 2
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'local build failed: required command is missing: %s\n' "$1" >&2
    exit 1
  fi
}

require_macos() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    printf 'local build failed: the app runtime currently supports macOS only\n' >&2
    exit 1
  fi
}

build_runtime() {
  local cargo_arguments=(build --locked)
  if [[ "${build_profile}" == "release" ]]; then
    cargo_arguments+=(--release)
  fi
  CARGO_TARGET_DIR="${target_root}" cargo "${cargo_arguments[@]}" \
    -p my-supervisor-app-daemon \
    -p my-supervisor-app-cli \
    -p my-supervisor-platform-macos
}

build_debug_app() {
  require_macos
  require_command pnpm
  cargo tauri --version >/dev/null
  build_runtime
  (
    cd "${workspace_root}/crates/desktop"
    CARGO_TARGET_DIR="${target_root}" cargo tauri build --debug --no-sign --bundles app
  )
  printf 'local debug app: %s/debug/bundle/macos/my-supervisor.app\n' "${target_root}"
}

build_release_app() {
  local host_target release_target="aarch64-apple-darwin"
  local artifact_root

  require_macos
  require_command pnpm
  cargo tauri --version >/dev/null
  host_target="$(rustc -vV | sed -n 's/^host: //p')"

  scripts/release/capture-source-provenance.sh create "${target_root}/source-manifest.tsv"
  if [[ "${host_target}" == "${release_target}" ]]; then
    unset CARGO_BUILD_TARGET
    artifact_root="${target_root}/release"
    CARGO_TARGET_DIR="${target_root}" cargo msv-release --locked
    CARGO_TARGET_DIR="${target_root}" scripts/release/prepare-macos-sidecars.sh
    (
      cd "${workspace_root}/crates/desktop"
      CARGO_TARGET_DIR="${target_root}" cargo tauri build --no-sign --bundles app
    )
  else
    export CARGO_BUILD_TARGET="${release_target}"
    artifact_root="${target_root}/${release_target}/release"
    CARGO_TARGET_DIR="${target_root}" cargo msv-release --locked
    CARGO_TARGET_DIR="${target_root}" scripts/release/prepare-macos-sidecars.sh
    (
      cd "${workspace_root}/crates/desktop"
      CARGO_TARGET_DIR="${target_root}" cargo tauri build \
        --target "${release_target}" --no-sign --bundles app
    )
  fi
  scripts/release/capture-source-provenance.sh verify "${target_root}/source-manifest.tsv"
  printf 'local release app: %s/bundle/macos/my-supervisor.app\n' "${artifact_root}"
}

require_command cargo
require_command rustc
[[ "${build_target}" == "cli" || "${build_target}" == "app" ]] || usage
[[ "${build_profile}" == "debug" || "${build_profile}" == "release" ]] || usage
cd "${workspace_root}"

case "${build_target}:${build_profile}" in
  cli:debug|cli:release)
    build_runtime
    ;;
  app:debug)
    build_debug_app
    ;;
  app:release)
    build_release_app
    ;;
esac
