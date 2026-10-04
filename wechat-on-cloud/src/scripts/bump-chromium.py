#!/usr/bin/env python3
"""把实例镜像锚定的 Chromium 升到 Debian bookworm 当前最新的构建（或指定版本）。

Chromium 必须锚定（架构守则 R1）：docker/Dockerfile 从 snapshot.debian.org 的固定快照安装指定版本，
因为新版本在本容器栈上崩过不止一次（149 断言收紧、150 非首次启动必崩）。以前升级要手工去快照站翻版本号和
时间戳，版本就一直停在旧的上面。本脚本只做机械的部分：

  1. 找版本：bookworm（~deb12uN）最新的 chromium，或命令行指定的版本；
  2. 找快照：chromium 与 chromium-common 在 amd64、arm64 上最晚进入 debian-security 的那次快照；
  3. 核对：下载该快照两个架构的 Packages 索引，确认这两个包的这个版本确实都在；
  4. 改 docker/Dockerfile 里的 CHROMIUM_SNAPSHOT / CHROMIUM_VERSION 两行。

改完仍须按 doc/dev/发布门禁.md 构建实例镜像、跑完全部探针（尤其 ② 同一 profile 第二次启动），
并实际打开一个 Chromium 实例、重启一次再看画面，才能合并。

用法：
  ./scripts/bump-chromium.py                             # 升到 bookworm 最新
  ./scripts/bump-chromium.py 154.0.8037.57-1~deb12u1     # 指定版本
  ./scripts/bump-chromium.py --check                     # 只看当前锚定的版本落后多少，不改文件
"""
import json
import lzma
import re
import sys
import urllib.request
from pathlib import Path

API = 'https://snapshot.debian.org/mr'
ARCHIVE = 'https://snapshot.debian.org/archive/debian-security'
PACKAGES = ('chromium', 'chromium-common')  # Dockerfile 装 chromium，它硬依赖同版本的 chromium-common
ARCHES = ('amd64', 'arm64')  # 实例镜像是双架构
DOCKERFILE = Path(__file__).resolve().parent.parent / 'docker' / 'Dockerfile'
BOOKWORM = re.compile(r'^(\d+(?:\.\d+)+)-(\d+)~deb12u(\d+)$')


def get(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=120) as r:
        return r.read()


def version_key(v: str):
    m = BOOKWORM.match(v)
    return (tuple(int(x) for x in m.group(1).split('.')), int(m.group(2)), int(m.group(3)))


def latest_bookworm() -> str:
    versions = [r['version'] for r in json.loads(get(f'{API}/package/chromium/'))['result']]
    return max((v for v in versions if BOOKWORM.match(v)), key=version_key)


def snapshot_for(version: str) -> str:
    stamps = []
    for pkg in PACKAGES:
        data = json.loads(get(f'{API}/binary/{pkg}/{urllib.request.quote(version)}/binfiles?fileinfo=1'))
        for arch in ARCHES:
            files = [r for r in data['result'] if r['architecture'] == arch]
            seen = [fi['first_seen'] for r in files for fi in data['fileinfo'].get(r['hash'], []) if fi['archive_name'] == 'debian-security']
            if not seen:
                sys.exit(f'✗ {pkg} {version} 在 debian-security 里没有 {arch} 构建')
            stamps.append(min(seen))
    return max(stamps)


def verify(snapshot: str, version: str) -> None:
    for arch in ARCHES:
        index = lzma.decompress(get(f'{ARCHIVE}/{snapshot}/dists/bookworm-security/main/binary-{arch}/Packages.xz')).decode('utf-8', 'replace')
        have = {(m.group(1), m.group(2)) for m in re.finditer(r'^Package: (\S+)\n(?:.+\n)*?Version: (\S+)', index, re.M)}
        missing = [p for p in PACKAGES if (p, version) not in have]
        if missing:
            sys.exit(f'✗ 快照 {snapshot} 的 {arch} 索引里没有 {", ".join(missing)} {version}')
        print(f'  ✓ {arch}：{", ".join(PACKAGES)} {version} 都在快照 {snapshot} 里')


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    text = DOCKERFILE.read_text(encoding='utf-8')
    cur_snap = re.search(r'^ARG CHROMIUM_SNAPSHOT=(\S+)$', text, re.M).group(1)
    cur_ver = re.search(r'^ARG CHROMIUM_VERSION=(\S+)$', text, re.M).group(1)
    newest = latest_bookworm()
    print(f'当前锚定：{cur_ver}（快照 {cur_snap}）')
    print(f'bookworm 最新：{newest}')
    if '--check' in sys.argv:
        return
    version = args[0] if args else newest
    if not BOOKWORM.match(version):
        sys.exit(f'✗ {version} 不是 bookworm 的构建（应形如 154.0.8037.57-1~deb12u1）')
    if version == cur_ver:
        print('已是该版本，无需改动')
        return
    snapshot = snapshot_for(version)
    verify(snapshot, version)
    text = re.sub(r'^ARG CHROMIUM_SNAPSHOT=\S+$', f'ARG CHROMIUM_SNAPSHOT={snapshot}', text, flags=re.M)
    text = re.sub(r'^ARG CHROMIUM_VERSION=\S+$', f'ARG CHROMIUM_VERSION={version}', text, flags=re.M)
    DOCKERFILE.write_text(text, encoding='utf-8')
    print(f'已改 docker/Dockerfile：{cur_ver} → {version}（快照 {cur_snap} → {snapshot}）')
    print('下一步：按 doc/dev/发布门禁.md 构建实例镜像并跑完全部探针，再实际打开 Chromium 实例、重启一次看画面。')


if __name__ == '__main__':
    main()
