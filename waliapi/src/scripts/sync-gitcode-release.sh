#!/usr/bin/env bash
#
# 自动同步 GitHub Release 到 GitCode
#
# 用法：
#   ./scripts/sync-gitcode-release.sh <github_tag> [gitcode_tag] [gitcode_token] [repo]
#
# 示例：
#   # 同步 WaLiCode-v0.6.5 到 GitCode 的 v0.6.5（自动检测仓库）
#   GITCODE_TOKEN=xxxx ./scripts/sync-gitcode-release.sh WaLiCode-v0.6.5 v0.6.5
#
#   # 指定仓库
#   ./scripts/sync-gitcode-release.sh WaLiCode-v0.6.5 v0.6.5 gpy_xxxxx fuzhengwei/WaLiCode
#
# 功能：
#   1. 从 GitHub Release 下载所有产物
#   2. 生成 GitCode 版 latest.json（替换 URL 域名）
#   3. 通过 GitCode API 创建 Release（含 make_latest=true）
#   4. 通过 GitCode OBS PUT 方式上传所有产物到 Release（支持大文件）
#   5. 验证上传结果
#
# 依赖：curl, python3
#
set -euo pipefail

# ─── 颜色 ───
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log()   { echo -e "${GREEN}[$(date +%H:%M:%S)]${NC} $*"; }
warn()  { echo -e "${YELLOW}[$(date +%H:%M:%S)] WARN:${NC} $*"; }
error() { echo -e "${RED}[$(date +%H:%M:%S)] ERROR:${NC} $*" >&2; }
info()  { echo -e "${BLUE}[$(date +%H:%M:%S)]${NC} $*"; }

# ─── 参数检查 ───
if [ $# -lt 1 ]; then
    echo "用法: $0 <github_tag> [gitcode_tag] [gitcode_token] [repo]"
    echo ""
    echo "参数:"
    echo "  github_tag    GitHub Release 的 tag 名 (如 WaLiCode-v0.6.5)"
    echo "  gitcode_tag   GitCode Release 的 tag 名 (默认与 github_tag 相同)"
    echo "  gitcode_token GitCode Personal Access Token (或设置 GITCODE_TOKEN 环境变量)"
    echo "  repo          仓库全名 owner/repo (默认自动检测 git remote origin)"
    echo ""
    echo "示例:"
    echo "  $0 WaLiCode-v0.6.5 v0.6.5"
    echo "  $0 WaLiCode-v0.6.5 v0.6.5 gpy_xxxxx fuzhengwei/WaLiCode"
    echo "  GITCODE_TOKEN=gpy_xxx $0 WaLiCode-v0.6.5"
    exit 1
fi

GH_TAG="$1"
GC_TAG="${2:-$GH_TAG}"
GC_TOKEN="${3:-${GITCODE_TOKEN:-}}"
REPO_ARG="${4:-}"

if [ -z "$GC_TOKEN" ]; then
    error "缺少 GitCode Token！"
    echo ""
    echo "获取方式："
    echo "  1. 登录 https://gitcode.com"
    echo "  2. 进入 设置 → 私人令牌 (Personal Access Tokens)"
    echo "  3. 创建新令牌，勾选 api scope"
    echo "  4. 设置环境变量: export GITCODE_TOKEN=gpy_xxxxx"
    echo "  5. 重新运行此脚本"
    exit 1
fi

# ─── 配置 ───
# 自动检测仓库：优先用参数，其次从 git remote origin 获取
if [ -n "$REPO_ARG" ]; then
    REPO="$REPO_ARG"
else
    REMOTE_URL=$(git -C "$(git rev-parse --show-toplevel 2>/dev/null || echo .)" remote get-url origin 2>/dev/null || echo "")
    if [ -n "$REMOTE_URL" ]; then
        # 支持 https://github.com/owner/repo.git 和 git@github.com:owner/repo.git
        REPO=$(echo "$REMOTE_URL" | sed -E 's#(https://[^/]+/|git@[^:]+:)(.+)\.git$#\2#' | sed -E 's#(https://[^/]+/|git@[^:]+:)(.+)$#\2#')
    else
        error "无法自动检测仓库，请通过第 4 个参数指定 (如 fuzhengwei/WaLiCode)"
        exit 1
    fi
fi

GC_API="https://gitcode.com/api/v5"
GH_API="https://api.github.com/repos"
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

log "仓库: $REPO"

# ─── 步骤 1: 获取 GitHub Release 信息 ───
log "步骤 1/5: 获取 GitHub Release 信息"
log "  Tag: $GH_TAG"

GH_RELEASE=$(curl -sf --max-time 15 \
    -H "Accept: application/vnd.github+json" \
    -H "User-Agent: sync-gitcode-release" \
    "$GH_API/$REPO/releases/tags/$GH_TAG" 2>/dev/null)

if [ -z "$GH_RELEASE" ]; then
    error "GitHub Release $GH_TAG 不存在"
    exit 1
fi

# 提取 release 信息
GH_NAME=$(echo "$GH_RELEASE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('name',''))")
GH_BODY=$(echo "$GH_RELEASE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('body',''))")
TARGET_COMMITISH=$(echo "$GH_RELEASE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('target_commitish','main'))")

log "  Name: $GH_NAME"
log "  Body: ${GH_BODY:0:60}..."

# 获取产物列表
ASSETS_JSON=$(echo "$GH_RELEASE" | python3 -c "
import sys, json
d = json.load(sys.stdin)
for a in d.get('assets', []):
    print(json.dumps({'name': a['name'], 'url': a['browser_download_url'], 'size': a['size']}))
")

ASSET_COUNT=$(echo "$ASSETS_JSON" | wc -l | tr -d ' ')
log "  产物数量: $ASSET_COUNT"

# ─── 步骤 2: 下载 GitHub Release 产物 ───
log "步骤 2/5: 下载 GitHub Release 产物"

declare -a ASSET_FILES  # 存储下载的文件路径

while IFS= read -r line; do
    [ -z "$line" ] && continue
    name=$(echo "$line" | python3 -c "import sys,json; print(json.load(sys.stdin)['name'])")
    url=$(echo "$line" | python3 -c "import sys,json; print(json.load(sys.stdin)['url'])")
    size=$(echo "$line" | python3 -c "import sys,json; print(json.load(sys.stdin)['size'])")
    
    outfile="$TMP_DIR/$name"
    log "  下载 $name ($(numfmt --to=iec $size 2>/dev/null || echo "${size}B"))"
    
    if curl -sfL --max-time 120 -H "User-Agent: sync-gitcode-release" -o "$outfile" "$url" 2>/dev/null; then
        log "    ✓ 下载成功"
        ASSET_FILES+=("$name")
    else
        warn "    ✗ 下载失败: $name，跳过"
    fi
done <<< "$ASSETS_JSON"

log "  已下载 ${#ASSET_FILES[@]}/$ASSET_COUNT 个产物"

# ─── 步骤 3: 生成 GitCode 版 latest.json ───
log "步骤 3/5: 生成 GitCode 版 latest.json"

LATEST_JSON="$TMP_DIR/latest.json"
if [ -f "$LATEST_JSON" ]; then
    python3 -c "
import json

with open('$LATEST_JSON') as f:
    data = json.load(f)

# 替换所有 URL 中的 github.com 为 gitcode.com
for platform in data.get('platforms', {}):
    url = data['platforms'][platform]['url']
    url = url.replace('https://github.com', 'https://gitcode.com')
    data['platforms'][platform]['url'] = url

with open('$LATEST_JSON', 'w') as f:
    json.dump(data, f, indent=2, ensure_ascii=False)
    f.write('\n')
"
    log "  ✓ latest.json URL 已替换为 gitcode.com"
    # 确保 latest.json 在上传列表中（它是 GitHub Release 的 asset，已经在 ASSET_FILES 里）
else
    warn "  latest.json 不存在，跳过"
fi

# ─── 步骤 4: 创建 GitCode Release ───
log "步骤 4/5: 创建 GitCode Release"
log "  Tag: $GC_TAG"

# 检查 release 是否已存在
EXISTING=$(curl -sf --max-time 10 \
    -H "Accept: application/json" \
    -H "private-token: $GC_TOKEN" \
    "$GC_API/repos/$REPO/releases/tags/$GC_TAG" 2>/dev/null || echo "")

if [ -n "$EXISTING" ] && echo "$EXISTING" | grep -q "tag_name"; then
    warn "  Release $GC_TAG 已存在，将更新"
    
    # 更新已有 release
    GC_RESPONSE=$(echo "$GH_BODY" | python3 -c "
import sys, json, urllib.request

body = sys.stdin.read()
data = json.dumps({
    'tag_name': '$GC_TAG',
    'name': '$GH_NAME',
    'body': body,
    'target_commitish': '$TARGET_COMMITISH',
    'make_latest': 'true'
}).encode()

req = urllib.request.Request(
    '$GC_API/repos/$REPO/releases/$GC_TAG',
    data=data, method='PATCH',
    headers={
        'Accept': 'application/json',
        'private-token': '$GC_TOKEN',
        'Content-Type': 'application/json'
    }
)
try:
    with urllib.request.urlopen(req, timeout=15) as r:
        print(r.read().decode())
except Exception as e:
    print(f'ERROR: {e}', file=sys.stderr)
" 2>/dev/null || echo "")
else
    # 创建新 release
    GC_RESPONSE=$(echo "$GH_BODY" | python3 -c "
import sys, json, urllib.request

body = sys.stdin.read()
data = json.dumps({
    'tag_name': '$GC_TAG',
    'name': '$GH_NAME',
    'body': body,
    'target_commitish': '$TARGET_COMMITISH',
    'prerelease': False,
    'make_latest': 'true'
}).encode()

req = urllib.request.Request(
    '$GC_API/repos/$REPO/releases',
    data=data, method='POST',
    headers={
        'Accept': 'application/json',
        'private-token': '$GC_TOKEN',
        'Content-Type': 'application/json'
    }
)
try:
    with urllib.request.urlopen(req, timeout=15) as r:
        print(r.read().decode())
except Exception as e:
    print(f'ERROR: {e}', file=sys.stderr)
" 2>/dev/null || echo "")
fi

if [ -z "$GC_RESPONSE" ]; then
    error "创建/更新 GitCode Release 失败"
    exit 1
fi

log "  ✓ GitCode Release 已创建/更新"

# ─── 步骤 4b: 设置为最新版本 ───
log "步骤 4b: 设置 Release 为最新版本"

# 尝试通过 PATCH make_latest 参数设置为最新（Gitea 1.18+ 兼容）
MAKE_LATEST_RESULT=$(python3 -c "
import json, urllib.request, sys

data = json.dumps({'make_latest': 'true'}).encode()
req = urllib.request.Request(
    '$GC_API/repos/$REPO/releases/$GC_TAG',
    data=data, method='PATCH',
    headers={
        'Accept': 'application/json',
        'private-token': '$GC_TOKEN',
        'Content-Type': 'application/json'
    }
)
try:
    with urllib.request.urlopen(req, timeout=15) as r:
        resp = json.loads(r.read().decode())
        status = resp.get('release_status', 'unknown')
        print(f'OK:{status}')
except urllib.error.HTTPError as e:
    print(f'HTTP_{e.code}')
except Exception as e:
    print(f'ERR_{e}')
" 2>/dev/null)

if [[ "$MAKE_LATEST_RESULT" == OK:* ]]; then
    status=${MAKE_LATEST_RESULT#OK:}
    if [[ "$status" == "latest" ]]; then
        log "  ✓ 已设置为最新版本 (release_status=latest)"
    else
        warn "  make_latest 参数已发送，但 release_status=$status"
    fi
else
    warn "  make_latest 参数可能不被支持: $MAKE_LATEST_RESULT"
    warn "  可手动在 GitCode 网页上设置最新版本"
fi

# ─── 步骤 5: 上传产物到 GitCode Release ───
log "步骤 5/5: 上传产物到 GitCode Release"

SUCCESS=0
FAILED=0

for filename in "${ASSET_FILES[@]}"; do
    filepath="$TMP_DIR/$filename"
    
    if [ ! -f "$filepath" ]; then
        warn "  ✗ 文件不存在: $filename"
        FAILED=$((FAILED + 1))
        continue
    fi
    
    filesize=$(stat -f%z "$filepath" 2>/dev/null || stat -c%s "$filepath" 2>/dev/null)
    log "  上传 $filename ($(numfmt --to=iec "$filesize" 2>/dev/null || echo "${filesize}B"))"
    
    # 主方式: 通过 upload_url 获取 OBS 预签名 URL，PUT 上传（支持大文件，无类型限制）
    UPLOAD_RESULT=$(python3 -c "
import json, urllib.request, ssl, sys

# 步骤 5a: 获取 OBS 预签名上传 URL
req = urllib.request.Request(
    '$GC_API/repos/$REPO/releases/$GC_TAG/upload_url?file_name=$filename',
    headers={
        'Accept': 'application/json',
        'private-token': '$GC_TOKEN'
    }
)
try:
    with urllib.request.urlopen(req, timeout=15) as r:
        info = json.loads(r.read())
except Exception as e:
    print(f'GET_URL_FAILED: {e}')
    sys.exit(0)

obs_url = info.get('url', '')
obs_headers = info.get('headers', {})

if not obs_url:
    print('NO_URL')
    sys.exit(0)

# 步骤 5b: PUT 上传到 OBS
with open('$filepath', 'rb') as f:
    fdata = f.read()

h = dict(obs_headers)
h['Content-Type'] = 'application/octet-stream'

ctx = ssl.create_default_context()
ctx.check_hostname = False
ctx.verify_mode = ssl.CERT_NONE

req = urllib.request.Request(obs_url, data=fdata, method='PUT', headers=h)
try:
    with urllib.request.urlopen(req, timeout=120, context=ctx) as r:
        print(f'OK_{r.status}')
except urllib.error.HTTPError as e:
    print(f'HTTP_{e.code}')
except Exception as e:
    print(f'ERR_{e}')
" 2>/dev/null)
    
    if [[ "$UPLOAD_RESULT" == OK_* ]]; then
        log "    ✓ 上传成功 (OBS PUT)"
        SUCCESS=$((SUCCESS + 1))
        # 上传成功后删除本地临时文件
        rm -f "$filepath"
        continue
    else
        warn "    OBS PUT 失败: $UPLOAD_RESULT，尝试 fallback..."
    fi
    
    # Fallback: 小文件用 file/upload 端点 (20MB 限制)
    FILESIZE_KB=$((filesize / 1024))
    if [ "$FILESIZE_KB" -lt 20480 ]; then
        UPLOAD_RESULT=$(curl -sf --max-time 120 -X POST \
            -H "Accept: application/json" \
            -H "private-token: $GC_TOKEN" \
            -F "file=@$filepath" \
            "$GC_API/repos/$REPO/file/upload" 2>/dev/null || echo "")
        
        if [ -n "$UPLOAD_RESULT" ]; then
            log "    ✓ 上传成功 (file/upload)"
            SUCCESS=$((SUCCESS + 1))
            # 上传成功后删除本地临时文件
            rm -f "$filepath"
            continue
        fi
    else
        warn "    文件超过 20MB，file/upload 不可用"
    fi
    
    error "    ✗ 上传失败: $filename"
    FAILED=$((FAILED + 1))
done

# 清理临时目录中残留的文件
log "  清理本地临时文件..."
rm -rf "$TMP_DIR"

# ─── 验证 ───
log ""
log "=== 同步结果 ==="
log "  成功: $SUCCESS"
if [ "$FAILED" -gt 0 ]; then
    warn "  失败: $FAILED"
fi

# 获取 GitCode release 最终状态
log ""
log "验证 GitCode Release..."
GC_FINAL=$(curl -sf --max-time 10 \
    -H "Accept: application/json" \
    -H "private-token: $GC_TOKEN" \
    "$GC_API/repos/$REPO/releases/tags/$GC_TAG" 2>/dev/null || echo "")

if [ -n "$GC_FINAL" ]; then
    echo "$GC_FINAL" | python3 -c "
import sys, json
d = json.load(sys.stdin)
print(f'  Tag: {d.get(\"tag_name\")}')
print(f'  Name: {d.get(\"name\")}')
print(f'  Status: {d.get(\"release_status\")}')
print('  Assets:')
for a in d.get('assets', []):
    print(f'    - {a[\"name\"]} (type={a[\"type\"]}, id={a.get(\"id\", \"N/A\")})')
" 2>/dev/null
fi

log ""
log "✅ 同步完成！"
log "  GitHub: https://github.com/$REPO/releases/tag/$GH_TAG"
log "  GitCode: https://gitcode.com/$REPO/releases/tag/$GC_TAG"

if [ "$FAILED" -gt 0 ]; then
    warn "有 $FAILED 个文件上传失败，可能需要手动上传"
    warn "  手动上传: https://gitcode.com/$REPO/releases"
    exit 1
fi
