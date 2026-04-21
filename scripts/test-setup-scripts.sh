#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly EXPECTED_PROTOTOOLS_CONTENT=$'node = "24.14.0"\nrust = "1.94.1"\npnpm = "10.11.0"'
readonly EXPECTED_WORKSPACE_CONTENT=$'projects:\n  - "apps/*"\n  - "packages/*"\n  - "crates/*"\n\nvcs:\n  defaultBranch: "main"\n  provider: "other"'
readonly EXPECTED_TOOLCHAINS_CONTENT=$'node: {}\nrust: {}'

TEMPORARY_DIRECTORY=""

cleanup() {
  if [[ -n "${TEMPORARY_DIRECTORY}" ]] && [[ -d "${TEMPORARY_DIRECTORY}" ]]; then
    rm -rf "${TEMPORARY_DIRECTORY}"
  fi
}

fail() {
  echo "실패: $*" >&2
  exit 1
}

assert_file_exists() {
  local file_path="$1"

  [[ -f "${file_path}" ]] || fail "${file_path} 파일이 없습니다."
}

assert_directory_exists() {
  local directory_path="$1"

  [[ -d "${directory_path}" ]] || fail "${directory_path} 디렉터리가 없습니다."
}

assert_file_content() {
  local file_path="$1"
  local expected_content="$2"
  local actual_content

  assert_file_exists "${file_path}"
  actual_content="$(<"${file_path}")"

  [[ "${actual_content}" == "${expected_content}" ]] || {
    echo "실패: ${file_path} 내용이 예상과 다릅니다." >&2
    echo "예상값:" >&2
    printf '%s\n' "${expected_content}" >&2
    echo "실제값:" >&2
    printf '%s\n' "${actual_content}" >&2
    exit 1
  }
}

assert_file_contains() {
  local file_path="$1"
  local expected_text="$2"

  assert_file_exists "${file_path}"
  grep -Fq -- "${expected_text}" "${file_path}" || fail "${file_path} 파일에 '${expected_text}' 내용이 없습니다."
}

create_workspace() {
  local workspace_path="$1"

  mkdir -p "${workspace_path}/scripts"
  install -m 0755 "${REPOSITORY_ROOT}/scripts/setup-moon.sh" "${workspace_path}/scripts/setup-moon.sh"
  install -m 0755 "${REPOSITORY_ROOT}/scripts/setup-proto.sh" "${workspace_path}/scripts/setup-proto.sh"
}

create_mock_binaries() {
  local mock_binary_directory="$1"

  mkdir -p "${mock_binary_directory}"

  cat <<'EOF' > "${mock_binary_directory}/moon"
#!/usr/bin/env bash

set -euo pipefail

printf 'moon %s\n' "$*" >> "${MOCK_TOOL_LOG_FILE:?}"
EOF

  cat <<'EOF' > "${mock_binary_directory}/proto"
#!/usr/bin/env bash

set -euo pipefail

printf 'proto %s\n' "$*" >> "${MOCK_TOOL_LOG_FILE:?}"

if [[ "${1:-}" == "install" ]] && [[ "${2:-}" == "--yes" ]] && [[ "${MOCK_PROTO_MUTATE_PROTOTOOLS:-false}" == "true" ]]; then
  cat <<'EOC' > .prototools
node = "0.0.0"
rust = "0.0.0"
pnpm = "0.0.0"
EOC
fi
EOF

  chmod +x "${mock_binary_directory}/moon" "${mock_binary_directory}/proto"
}

run_in_workspace() {
  local workspace_path="$1"
  local log_file_path="$2"
  local mutate_prototools="${3:-false}"
  shift 3

  (
    cd "${workspace_path}"
    PATH="${TEMPORARY_DIRECTORY}/bin:${PATH}" \
      MOCK_TOOL_LOG_FILE="${log_file_path}" \
      MOCK_PROTO_MUTATE_PROTOTOOLS="${mutate_prototools}" \
      "$@"
  )
}

test_setup_moon_creates_expected_files() {
  local workspace_path="${TEMPORARY_DIRECTORY}/moon-create"

  create_workspace "${workspace_path}"
  : > "${workspace_path}/tool.log"

  run_in_workspace "${workspace_path}" "${workspace_path}/tool.log" false bash "${workspace_path}/scripts/setup-moon.sh"

  assert_directory_exists "${workspace_path}/apps"
  assert_directory_exists "${workspace_path}/packages"
  assert_directory_exists "${workspace_path}/crates"
  assert_file_content "${workspace_path}/.moon/workspace.yml" "${EXPECTED_WORKSPACE_CONTENT}"
  assert_file_content "${workspace_path}/.moon/toolchains.yml" "${EXPECTED_TOOLCHAINS_CONTENT}"
}

test_setup_moon_preserves_existing_configuration() {
  local workspace_path="${TEMPORARY_DIRECTORY}/moon-reuse"

  create_workspace "${workspace_path}"
  mkdir -p "${workspace_path}/.moon"
  : > "${workspace_path}/tool.log"

  cat <<'EOF' > "${workspace_path}/.moon/workspace.yml"
projects:
  - "custom/*"
EOF

  cat <<'EOF' > "${workspace_path}/.moon/toolchains.yml"
deno: {}
EOF

  run_in_workspace "${workspace_path}" "${workspace_path}/tool.log" false bash "${workspace_path}/scripts/setup-moon.sh"

  assert_directory_exists "${workspace_path}/apps"
  assert_directory_exists "${workspace_path}/packages"
  assert_directory_exists "${workspace_path}/crates"
  assert_file_content "${workspace_path}/.moon/workspace.yml" $'projects:\n  - "custom/*"'
  assert_file_content "${workspace_path}/.moon/toolchains.yml" 'deno: {}'
}

test_setup_proto_creates_expected_prototools_file() {
  local workspace_path="${TEMPORARY_DIRECTORY}/proto-create"

  create_workspace "${workspace_path}"
  : > "${workspace_path}/tool.log"

  run_in_workspace "${workspace_path}" "${workspace_path}/tool.log" false bash "${workspace_path}/scripts/setup-proto.sh"

  assert_file_content "${workspace_path}/.prototools" "${EXPECTED_PROTOTOOLS_CONTENT}"
  assert_file_contains "${workspace_path}/tool.log" 'proto install --yes'
}

test_setup_proto_preserves_existing_prototools_file() {
  local workspace_path="${TEMPORARY_DIRECTORY}/proto-preserve"

  create_workspace "${workspace_path}"
  : > "${workspace_path}/tool.log"

  cat <<'EOF' > "${workspace_path}/.prototools"
node = "18.20.0"
rust = "1.80.0"
pnpm = "9.0.0"
EOF

  run_in_workspace "${workspace_path}" "${workspace_path}/tool.log" false bash "${workspace_path}/scripts/setup-proto.sh"

  assert_file_content "${workspace_path}/.prototools" $'node = "18.20.0"\nrust = "1.80.0"\npnpm = "9.0.0"'
  assert_file_contains "${workspace_path}/tool.log" 'proto install --yes'
}

test_setup_proto_restores_expected_prototools_file_after_proto_install() {
  local workspace_path="${TEMPORARY_DIRECTORY}/proto-restore"

  create_workspace "${workspace_path}"
  : > "${workspace_path}/tool.log"
  printf '%s\n' "${EXPECTED_PROTOTOOLS_CONTENT}" > "${workspace_path}/.prototools"

  run_in_workspace "${workspace_path}" "${workspace_path}/tool.log" true bash "${workspace_path}/scripts/setup-proto.sh"

  assert_file_content "${workspace_path}/.prototools" "${EXPECTED_PROTOTOOLS_CONTENT}"
  assert_file_contains "${workspace_path}/tool.log" 'proto install --yes'
}

main() {
  TEMPORARY_DIRECTORY="$(mktemp -d)"
  trap cleanup EXIT

  create_mock_binaries "${TEMPORARY_DIRECTORY}/bin"

  test_setup_moon_creates_expected_files
  test_setup_moon_preserves_existing_configuration
  test_setup_proto_creates_expected_prototools_file
  test_setup_proto_preserves_existing_prototools_file
  test_setup_proto_restores_expected_prototools_file_after_proto_install

  echo "모든 setup 스크립트 테스트가 통과했습니다."
}

main "$@"
