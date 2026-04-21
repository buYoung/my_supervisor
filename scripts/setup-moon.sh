#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly MOON_DIRECTORY="${REPOSITORY_ROOT}/.moon"
readonly WORKSPACE_FILE="${MOON_DIRECTORY}/workspace.yml"
readonly TOOLCHAINS_FILE="${MOON_DIRECTORY}/toolchains.yml"

ensure_moon_is_installed() {
  if command -v moon >/dev/null 2>&1; then
    return
  fi

  echo "moon 명령을 찾을 수 없습니다. 먼저 moon을 설치한 뒤 다시 실행하세요." >&2
  echo "설치 안내: https://moonrepo.dev/docs/install" >&2
  exit 1
}

ensure_workspace_directories() {
  mkdir -p \
    "${REPOSITORY_ROOT}/apps" \
    "${REPOSITORY_ROOT}/packages" \
    "${REPOSITORY_ROOT}/crates"
}

ensure_workspace_file() {
  if [[ -f "${WORKSPACE_FILE}" ]]; then
    echo ".moon/workspace.yml 파일을 재사용합니다."
    return
  fi

  mkdir -p "${MOON_DIRECTORY}"

  cat <<'EOF' > "${WORKSPACE_FILE}"
projects:
  - "apps/*"
  - "packages/*"
  - "crates/*"

vcs:
  defaultBranch: "main"
  provider: "other"
EOF

  echo ".moon/workspace.yml 파일을 생성했습니다."
}

ensure_toolchains_file() {
  if [[ -f "${TOOLCHAINS_FILE}" ]]; then
    echo ".moon/toolchains.yml 파일을 재사용합니다."
    return
  fi

  mkdir -p "${MOON_DIRECTORY}"

  cat <<'EOF' > "${TOOLCHAINS_FILE}"
node: {}
rust: {}
EOF

  echo ".moon/toolchains.yml 파일을 생성했습니다."
}

main() {
  ensure_moon_is_installed
  ensure_workspace_directories
  ensure_workspace_file
  ensure_toolchains_file
}

main "$@"
