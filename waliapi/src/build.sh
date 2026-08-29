#!/usr/bin/env bash
set -euo pipefail

# WaLiAPI Docker 镜像构建脚本
# 用法:
#   ./build.sh              # 构建当前平台镜像（最快，本地验证）
#   ./build.sh local        # 同上
#   ./build.sh amd64        # 仅构建 amd64，加载到本地（tag 带 -amd64 后缀）
#   ./build.sh arm64        # 仅构建 arm64，加载到本地（tag 带 -arm64 后缀）
#   ./build.sh both         # 依次构建 amd64 + arm64，都加载到本地（不同 tag 共存）
#   ./build.sh multi        # 构建多平台（amd64 + arm64），推送到 registry（需先 docker login）

IMAGE_NAME="fuzhengwei/waliapi"
IMAGE_TAG="0.2.5"
PLATFORMS="linux/amd64,linux/arm64"

# 国内 Debian 镜像源（HTTP，避免 runtime 阶段 ca-certificates 未装时 HTTPS 证书校验失败）
DEBIAN_MIRROR="http://mirrors.tuna.tsinghua.edu.cn/debian"
DEBIAN_SECURITY_MIRROR="http://mirrors.tuna.tsinghua.edu.cn/debian-security"

# 公共构建参数
BUILD_ARGS=(
  --build-arg "DEBIAN_MIRROR=${DEBIAN_MIRROR}"
  --build-arg "DEBIAN_SECURITY_MIRROR=${DEBIAN_SECURITY_MIRROR}"
)

MODE="${1:-local}"

case "$MODE" in
  local)
    echo "📦 构建当前平台镜像: ${IMAGE_NAME}:${IMAGE_TAG}"
    docker build "${BUILD_ARGS[@]}" \
      -t "${IMAGE_NAME}:${IMAGE_TAG}" \
      -f ./Dockerfile .
    ;;
  amd64)
    echo "📦 构建 amd64 镜像: ${IMAGE_NAME}:${IMAGE_TAG}-amd64"
    docker build --platform linux/amd64 "${BUILD_ARGS[@]}" \
      -t "${IMAGE_NAME}:${IMAGE_TAG}-amd64" \
      -f ./Dockerfile .
    ;;
  arm64)
    echo "📦 构建 arm64 镜像: ${IMAGE_NAME}:${IMAGE_TAG}-arm64"
    docker build --platform linux/arm64 "${BUILD_ARGS[@]}" \
      -t "${IMAGE_NAME}:${IMAGE_TAG}-arm64" \
      -f ./Dockerfile .
    ;;
  both)
    echo "📦 构建 amd64 镜像: ${IMAGE_NAME}:${IMAGE_TAG}-amd64"
    docker build --platform linux/amd64 "${BUILD_ARGS[@]}" \
      -t "${IMAGE_NAME}:${IMAGE_TAG}-amd64" \
      -f ./Dockerfile .
    echo "📦 构建 arm64 镜像: ${IMAGE_NAME}:${IMAGE_TAG}-arm64"
    docker build --platform linux/arm64 "${BUILD_ARGS[@]}" \
      -t "${IMAGE_NAME}:${IMAGE_TAG}-arm64" \
      -f ./Dockerfile .
    echo "✅ 两个架构镜像均已加载到本地:"
    docker images "${IMAGE_NAME}" --format "  {{.Repository}}:{{.Tag}}\t{{.Architecture}}\t{{.Size}}"
    ;;
  multi)
    echo "📦 构建多平台镜像并推送到 registry: ${IMAGE_NAME}:${IMAGE_TAG}"
    echo "   平台: ${PLATFORMS}"
    echo "   确保已 docker login 对应 registry"
    docker buildx build --push --platform "${PLATFORMS}" "${BUILD_ARGS[@]}" \
      -t "${IMAGE_NAME}:${IMAGE_TAG}" \
      -t "${IMAGE_NAME}:latest" \
      -f ./Dockerfile .
    ;;
  *)
    echo "用法: ./build.sh [local|amd64|arm64|both|multi]"
    echo "  local  - 构建当前平台，加载到本地（默认）"
    echo "  amd64  - 仅 amd64，加载到本地（tag: ${IMAGE_TAG}-amd64）"
    echo "  arm64  - 仅 arm64，加载到本地（tag: ${IMAGE_TAG}-arm64）"
    echo "  both   - amd64 + arm64 依次构建，都加载到本地"
    echo "  multi  - amd64+arm64 多平台，推送到 registry"
    exit 1
    ;;
esac

echo "✅ 完成"
