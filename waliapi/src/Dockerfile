# Build the Web 管理面板前端与 headless 服务端二进制（waliapi-web）。
# 不再构建桌面端：无窗口、无托盘，运行时无需 GTK/WebKit/Xvfb。
ARG NODE_IMAGE=node:22-bookworm
ARG RUNTIME_IMAGE=debian:bookworm-slim
ARG DEBIAN_MIRROR=http://deb.debian.org/debian
ARG DEBIAN_SECURITY_MIRROR=http://deb.debian.org/debian-security
ARG RUSTUP_MIRROR=https://rsproxy.cn
ARG CARGO_REGISTRY_MIRROR=sparse+https://rsproxy.cn/index/

FROM ${NODE_IMAGE} AS builder

ARG DEBIAN_MIRROR
ARG DEBIAN_SECURITY_MIRROR
ARG RUSTUP_MIRROR
ARG CARGO_REGISTRY_MIRROR

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_DIST_SERVER=${RUSTUP_MIRROR} \
    RUSTUP_UPDATE_ROOT=${RUSTUP_MIRROR}/rustup \
    PATH=/usr/local/cargo/bin:$PATH

# libwebkit2gtk-4.1-dev 等仅为编译期依赖（tauri/wry 的 sys crate 需要 pkg-config 探测），
# waliapi-web 运行时不引用任何 GUI 符号（--as-needed 下不会进入 NEEDED）。
RUN sed -i "s|http://deb.debian.org/debian-security|${DEBIAN_SECURITY_MIRROR}|g; s|http://deb.debian.org/debian|${DEBIAN_MIRROR}|g" /etc/apt/sources.list.d/debian.sources \
    && apt-get -o Acquire::Retries=5 -o Acquire::http::Timeout=30 update \
    && apt-get -o Acquire::Retries=5 -o Acquire::http::Timeout=30 install -y --no-install-recommends \
        build-essential \
        curl \
        file \
        libayatana-appindicator3-dev \
        libgtk-3-dev \
        libssl-dev \
        libwebkit2gtk-4.1-dev \
        librsvg2-dev \
        patchelf \
        pkg-config \
        xdg-utils \
    && rm -rf /var/lib/apt/lists/* \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path \
    && mkdir -p ${CARGO_HOME} \
    && printf '[source.crates-io]\nreplace-with = "mirror"\n[source.mirror]\nregistry = "%s"\n' "${CARGO_REGISTRY_MIRROR}" > ${CARGO_HOME}/config.toml

WORKDIR /app
RUN corepack enable

# 先拷贝依赖清单以利用镜像缓存（web 子包清单也需在内）
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY web/package.json web/package.json
RUN pnpm install --frozen-lockfile

COPY . .
# Web 管理面板（产出 web/dist，供 rust-embed 内嵌）→ 编译 headless 服务端
# --no-default-features 关闭 desktop-ui（文件对话框/自启/OAuth 打开浏览器），
# 避免二进制 NEEDED 引入 GTK/WebKit/DBus 动态库
RUN pnpm --filter waliapi-web build \
    && cargo build --manifest-path src-tauri/Cargo.toml --release --bin waliapi-web --no-default-features --features embed-web

# Runtime：纯 headless，无 GUI 依赖
FROM ${RUNTIME_IMAGE} AS runtime

ARG DEBIAN_MIRROR
ARG DEBIAN_SECURITY_MIRROR

ENV DEBIAN_FRONTEND=noninteractive \
    WALIAPI_SERVER_HOST=0.0.0.0 \
    WALIAPI_SERVER_PORT=8777 \
    XDG_DATA_HOME=/data \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8

# Runtime：headless 运行（不创建窗口/不需要显示服务器）。
# 注：waliapi-web 链接 tauri 库（wry 为 tauri 不可选依赖），二进制带 webkit/gtk/dbus 的
# NEEDED 动态库条目（仅加载需要，运行时从不调用），因此保留这几个运行库；
# Xvfb/VNC/noVNC/fluxbox/dbus-x11/字体等显示栈已全部移除。
RUN sed -i "s|http://deb.debian.org/debian-security|${DEBIAN_SECURITY_MIRROR}|g; s|http://deb.debian.org/debian|${DEBIAN_MIRROR}|g" /etc/apt/sources.list.d/debian.sources \
    && apt-get -o Acquire::Retries=5 -o Acquire::http::Timeout=30 update \
    && apt-get -o Acquire::Retries=5 -o Acquire::http::Timeout=30 install -y --no-install-recommends \
        ca-certificates \
        curl \
        libdbus-1-3 \
        libssl3 \
        libwebkit2gtk-4.1-0 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 waliapi \
    && mkdir -p /data \
    && chown -R waliapi:waliapi /data

COPY --from=builder /app/src-tauri/target/release/waliapi-web /usr/local/bin/waliapi-web

USER waliapi
WORKDIR /home/waliapi
VOLUME ["/data"]
EXPOSE 8777

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl --fail --silent "http://127.0.0.1:${WALIAPI_SERVER_PORT}/health" || exit 1

ENTRYPOINT ["/usr/local/bin/waliapi-web"]
CMD ["start"]
