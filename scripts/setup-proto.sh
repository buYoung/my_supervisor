#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly PROTOTOOLS_FILE="${REPOSITORY_ROOT}/.prototools"
readonly EXPECTED_NODE_VERSION="24.14.0"
readonly EXPECTED_RUST_VERSION="1.94.1"
readonly EXPECTED_PNPM_VERSION="10.11.0"

should_restore_prototools_file=false

ensure_proto_is_installed() {
  if command -v proto >/dev/null 2>&1; then
    return
  fi

  echo "proto 명령을 찾을 수 없습니다. 먼저 proto를 설치한 뒤 다시 실행하세요." >&2
  echo "설치 안내: https://moonrepo.dev/proto/install" >&2
  exit 1
}

get_expected_prototools_content() {
  printf 'node = "%s"\nrust = "%s"\npnpm = "%s"' \
    "${EXPECTED_NODE_VERSION}" \
    "${EXPECTED_RUST_VERSION}" \
    "${EXPECTED_PNPM_VERSION}"
}

write_expected_prototools_file() {
  cat <<EOF > "${PROTOTOOLS_FILE}"
node = "${EXPECTED_NODE_VERSION}"
rust = "${EXPECTED_RUST_VERSION}"
pnpm = "${EXPECTED_PNPM_VERSION}"
EOF
}

ensure_prototools_file() {
  local current_content
  local expected_content

  expected_content="$(get_expected_prototools_content)"

  if [[ -f "${PROTOTOOLS_FILE}" ]]; then
    echo ".prototools 파일을 재사용합니다."
    current_content="$(<"${PROTOTOOLS_FILE}")"

    if [[ "${current_content}" == "${expected_content}" ]]; then
      should_restore_prototools_file=true
    fi

    return
  fi

  write_expected_prototools_file
  should_restore_prototools_file=true
  echo ".prototools 파일을 생성했습니다."
}

restore_prototools_file_if_needed() {
  local current_content
  local expected_content

  if [[ "${should_restore_prototools_file}" != true ]] || [[ ! -f "${PROTOTOOLS_FILE}" ]]; then
    return
  fi

  current_content="$(<"${PROTOTOOLS_FILE}")"
  expected_content="$(get_expected_prototools_content)"

  if [[ "${current_content}" == "${expected_content}" ]]; then
    return
  fi

  write_expected_prototools_file
  echo "proto 실행 중 변경된 .prototools 파일을 기준 버전으로 복원했습니다."
}

install_tools() {
  (
    cd "${REPOSITORY_ROOT}"
    proto install --yes
  )
}

print_next_step() {
  echo
  echo "셸 프로필은 수정하지 않았습니다."
  echo "필요하면 아래 명령을 사용자가 직접 실행하세요:"
  echo "  proto setup"
}

main() {
  ensure_proto_is_installed
  ensure_prototools_file
  trap restore_prototools_file_if_needed EXIT
  install_tools
  print_next_step
}

main "$@"
