#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly DOCKERFILE_PATH="${REPOSITORY_ROOT}/docker/scripts-test.Dockerfile"
readonly IMAGE_NAME_PREFIX="my-supervisor-setup-scripts-test"

readonly UBUNTU_VERSIONS=(
  "20.04"
  "22.04"
  "24.04"
  "26.04"
)

run_test_for_ubuntu_version() {
  local ubuntu_version="$1"
  local image_tag="${IMAGE_NAME_PREFIX}:ubuntu-${ubuntu_version//./-}"

  echo
  echo "Ubuntu ${ubuntu_version} 테스트를 시작합니다."

  docker build \
    --no-cache \
    --progress=plain \
    --build-arg "UBUNTU_VERSION=${ubuntu_version}" \
    --file "${DOCKERFILE_PATH}" \
    --tag "${image_tag}" \
    "${REPOSITORY_ROOT}"

  echo "Ubuntu ${ubuntu_version} 테스트가 통과했습니다."
}

main() {
  local ubuntu_version

  for ubuntu_version in "${UBUNTU_VERSIONS[@]}"; do
    run_test_for_ubuntu_version "${ubuntu_version}"
  done

  echo
  echo "모든 Ubuntu 버전 테스트가 통과했습니다."
}

main "$@"
