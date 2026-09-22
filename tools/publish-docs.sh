#!/usr/bin/env bash
# 把「README + docs/」快照发布到公开文档仓库，源码不入库。
#
#   ./tools/publish-docs.sh              # 生成快照并推送
#   PUBLIC_REPO=owner/name ./tools/publish-docs.sh
#
# 快照规则：
#   - 只取 git 已跟踪的 README.md 与 docs/**（未提交的草稿一律不发布）
#   - 默认跳过内部过程文档 docs/superpowers/**、docs/codex/**、docs/plans/**，
#     并把指向它们的链接降级为纯文本
#     （PUBLISH_INTERNAL_DOCS=1 可一并公开）
#   - 路径脱敏：<repo 绝对路径> → <repo>，/Users/<me> → ~，并去掉本机用户名
#   - 在 README 顶部加「本仓库只发布文档」说明
#   - 独立 git 历史（不含源码提交），main 分支，force-push 覆盖
set -euo pipefail

PUBLIC_REPO="${PUBLIC_REPO:-yutayouguan/astro-agent-docs}"
PUBLISH_INTERNAL_DOCS="${PUBLISH_INTERNAL_DOCS:-0}"
INTERNAL_DOC_DIRS=(docs/superpowers/ docs/codex/ docs/plans/)
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOME_DIR="${HOME:-/Users/$(whoami)}"
BANNER='> **说明**：本仓库只发布 Astro Agent 的**文档与界面截图**，源代码不在此仓库公开。'
SOURCE_NOTE='> 下面的构建与运行命令需要 Astro Agent 的完整源码，本仓库只包含文档。'

WORK="$(mktemp -d)"
STAGE="$WORK/site"
cleanup() {
  local status=$?
  if [ "$status" -eq 0 ] && [ "${KEEP_STAGE:-0}" != "1" ]; then
    rm -rf "$WORK"
    echo "==> 已清理快照目录（KEEP_STAGE=1 可保留）"
  else
    echo "==> 快照保留在 ${WORK}"
  fi
  return "$status"
}
trap cleanup EXIT
echo "==> 快照目录：$STAGE"

mkdir -p "$STAGE"

# 只发布已提交内容：未跟踪文件（草稿、个人笔记）不进公开快照
LIST="$WORK/files.txt"
# core.quotePath=false：中文路径按原文输出，交给 rsync --files-from 解析
git -C "$REPO_ROOT" -c core.quotePath=false ls-files README.md docs >"$LIST"
if [ "$PUBLISH_INTERNAL_DOCS" != "1" ]; then
  for dir in "${INTERNAL_DOC_DIRS[@]}"; do
    grep -v "^${dir}" "$LIST" >"$LIST.tmp" || true
    mv "$LIST.tmp" "$LIST"
  done
fi
grep -v '/\.DS_Store$' "$LIST" >"$LIST.tmp" || true
mv "$LIST.tmp" "$LIST"
rsync -a --files-from="$LIST" "$REPO_ROOT/" "$STAGE/"

if [ "$PUBLISH_INTERNAL_DOCS" != "1" ]; then
  echo "==> 降级指向内部文档的链接（superpowers/、codex/、plans/）"
  python3 - "$STAGE" <<'PY'
import os, re, sys

stage = sys.argv[1]
link = re.compile(r"\[([^\]]*)\]\(([^)]+)\)")
internal = re.compile(r"(^|/)(superpowers|codex|plans)/")
hit = 0
for root, _dirs, files in os.walk(stage):
    for name in files:
        if not name.endswith(".md"):
            continue
        path = os.path.join(root, name)
        text = open(path, encoding="utf-8").read()

        def downgrade(match):
            global hit
            target = match.group(2)
            if not internal.search(target):
                return match.group(0)
            hit += 1
            return match.group(1)

        updated = link.sub(downgrade, text)
        if updated != text:
            open(path, "w", encoding="utf-8").write(updated)
print(f"    降级 {hit} 个链接")
PY
fi

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
