#!/usr/bin/env bash
# 把「README + docs/」快照发布到公开文档仓库，源码不入库。
#
#   ./tools/publish-docs.sh              # 生成快照并推送
#   PUBLIC_REPO=owner/name ./tools/publish-docs.sh
#
# 快照规则：
#   - 只取 README.md 与 docs/**（跳过 docs/images/raw 与 .DS_Store）
#   - 路径脱敏：<repo 绝对路径> → <repo>，/Users/<me> → ~，并去掉本机用户名
#   - 在 README 顶部加「本仓库只发布文档」说明
#   - 独立 git 历史（不含源码提交），main 分支，force-push 覆盖
set -euo pipefail

PUBLIC_REPO="${PUBLIC_REPO:-yutayouguan/astro-agent-docs}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOME_DIR="${HOME:-/Users/$(whoami)}"
BANNER='> **说明**：本仓库只发布 Astro Agent 的**文档与界面截图**，源代码不在此仓库公开。'
SOURCE_NOTE='> 下面的构建与运行命令需要 Astro Agent 的完整源码，本仓库只包含文档。'

STAGE="$(mktemp -d)"
cleanup() {
  local status=$?
  if [ "$status" -eq 0 ] && [ "${KEEP_STAGE:-0}" != "1" ]; then
    rm -rf "$STAGE"
    echo "==> 已清理快照目录（KEEP_STAGE=1 可保留）"
  else
    echo "==> 快照保留在 ${STAGE}"
  fi
  return "$status"
}
trap cleanup EXIT
echo "==> 快照目录：$STAGE"

mkdir -p "$STAGE/docs"
cp "$REPO_ROOT/README.md" "$STAGE/README.md"
rsync -a \
  --exclude 'images/raw/' \
  --exclude '.DS_Store' \
  "$REPO_ROOT/docs/" "$STAGE/docs/"

echo "==> 脱敏本机路径"
find "$STAGE" -type f \( -name '*.md' -o -name '*.json' -o -name '*.toml' -o -name '*.sh' -o -name '*.txt' \) -print0 |
  LC_ALL=C xargs -0 sed -i '' \
    -e "s|$REPO_ROOT|<repo>|g" \
    -e "s|$HOME_DIR|~|g"

echo "==> 插入公开说明"
python3 - "$STAGE/README.md" "$BANNER" "$SOURCE_NOTE" <<'PY'
import sys
path, banner, source_note = sys.argv[1], sys.argv[2], sys.argv[3]
text = open(path, encoding="utf-8").read()
if banner not in text:
    marker = "</div>\n"
    text = text.replace(marker, marker + "\n" + banner + "\n", 1)
if source_note not in text:
    heading = "## 快速开始\n"
    text = text.replace(heading, heading + "\n" + source_note + "\n", 1)
open(path, "w", encoding="utf-8").write(text)
PY

cd "$STAGE"
git init -q -b main
git add -A
git -c user.name="$(git -C "$REPO_ROOT" config user.name)" \
    -c user.email="$(git -C "$REPO_ROOT" config user.email)" \
    commit -q -m "docs: 发布 Astro Agent 文档快照（$(date +%Y-%m-%d)）"

if ! gh repo view "$PUBLIC_REPO" >/dev/null 2>&1; then
  echo "==> 创建公开仓库 $PUBLIC_REPO"
  gh repo create "$PUBLIC_REPO" --public --description "Astro Agent 文档与界面说明（源码不公开）"
fi

git remote add origin "https://github.com/$PUBLIC_REPO.git"
echo "==> 推送"
git push --force -u origin main

echo "==> 完成：https://github.com/$PUBLIC_REPO"
echo "    文件数：$(git ls-files | wc -l | tr -d ' ')  .git 大小：$(du -sh "$STAGE/.git" | cut -f1)"
