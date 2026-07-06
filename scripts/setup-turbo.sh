#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly ROOT_PACKAGE_FILE="${REPOSITORY_ROOT}/package.json"
readonly APP_PACKAGE_FILE="${REPOSITORY_ROOT}/apps/my_supervisor/package.json"
readonly PNPM_WORKSPACE_FILE="${REPOSITORY_ROOT}/pnpm-workspace.yaml"
readonly TURBO_FILE="${REPOSITORY_ROOT}/turbo.json"

ensure_pnpm_is_installed() {
  if command -v pnpm >/dev/null 2>&1; then
    return
  fi

  echo "pnpm 명령을 찾을 수 없습니다. 먼저 ./scripts/setup-proto.sh를 실행하세요." >&2
  exit 1
}

ensure_workspace_directories() {
  mkdir -p \
    "${REPOSITORY_ROOT}/apps/my_supervisor" \
    "${REPOSITORY_ROOT}/packages"
}

ensure_root_package_file() {
  if [[ -f "${ROOT_PACKAGE_FILE}" ]]; then
    echo "package.json 파일을 재사용합니다."
    return
  fi

  cat <<'EOF' > "${ROOT_PACKAGE_FILE}"
{
  "name": "my-supervisor",
  "version": "0.1.0",
  "private": true,
  "packageManager": "pnpm@10.11.0",
  "scripts": {
    "dev": "turbo run dev --filter=@my-supervisor/ui",
    "build": "turbo run build",
    "build:rust": "turbo run build --filter=@my-supervisor/my-supervisor",
    "build:ui": "turbo run build --filter=@my-supervisor/ui",
    "typecheck": "turbo run typecheck",
    "typecheck:rust": "turbo run typecheck --filter=@my-supervisor/my-supervisor",
    "typecheck:ui": "turbo run typecheck --filter=@my-supervisor/ui",
    "desktop:dev": "pnpm --filter @my-supervisor/my-supervisor desktop:dev",
    "desktop:build": "pnpm --filter @my-supervisor/my-supervisor desktop:build"
  },
  "devDependencies": {
    "turbo": "^2.10.3"
  }
}
EOF

  echo "package.json 파일을 생성했습니다."
}

ensure_app_package_file() {
  if [[ -f "${APP_PACKAGE_FILE}" ]]; then
    echo "apps/my_supervisor/package.json 파일을 재사용합니다."
    return
  fi

  cat <<'EOF' > "${APP_PACKAGE_FILE}"
{
  "name": "@my-supervisor/my-supervisor",
  "version": "0.1.0",
  "private": true,
  "scripts": {
    "build": "cargo build --workspace",
    "typecheck": "cargo check --workspace",
    "desktop:dev": "cargo tauri dev",
    "desktop:build": "cargo tauri build"
  }
}
EOF

  echo "apps/my_supervisor/package.json 파일을 생성했습니다."
}

ensure_pnpm_workspace_file() {
  if [[ -f "${PNPM_WORKSPACE_FILE}" ]]; then
    echo "pnpm-workspace.yaml 파일을 재사용합니다."
    return
  fi

  cat <<'EOF' > "${PNPM_WORKSPACE_FILE}"
packages:
  - "apps/*"
  - "packages/*"
EOF

  echo "pnpm-workspace.yaml 파일을 생성했습니다."
}

ensure_turbo_file() {
  if [[ -f "${TURBO_FILE}" ]]; then
    echo "turbo.json 파일을 재사용합니다."
    return
  fi

  cat <<'EOF' > "${TURBO_FILE}"
{
  "$schema": "https://turborepo.com/schema.json",
  "tasks": {
    "build": {
      "dependsOn": ["^build"],
      "outputs": ["dist/**", "target/**"]
    },
    "typecheck": {
      "dependsOn": ["^typecheck"],
      "outputs": []
    },
    "dev": {
      "cache": false,
      "persistent": true
    }
  }
}
EOF

  echo "turbo.json 파일을 생성했습니다."
}

print_next_step() {
  echo
  echo "의존성 설치가 필요하면 아래 명령을 실행하세요:"
  echo "  pnpm install"
}

main() {
  ensure_pnpm_is_installed
  ensure_workspace_directories
  ensure_root_package_file
  ensure_app_package_file
  ensure_pnpm_workspace_file
  ensure_turbo_file
  print_next_step
}

main "$@"
