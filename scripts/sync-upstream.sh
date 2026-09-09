#!/usr/bin/env bash
# Frame 上游同步 runbook（uuo310/Frame-zh-cn）。
# 作用：把"我们的提交栈"重放到新上游基线上；冲突处停下交给人裁决；裁完做对账+测试。
# 纪律：upstream remote 的 push 已 DISABLED（只读）；本脚本只 fetch/rebase/测试，不 push。
#       push 由人工在确认对账与测试通过后执行：git push origin zh-cn
set -euo pipefail

BASE_OLD=eefde7a          # 当前栈的上游基点；上游大版本切换时按需更新
BRANCH=zh-cn

echo "== 1. fetch 上游 =="
git fetch upstream

echo "== 2. 预判：上游动了哪些文件 =="
git diff --stat "${BASE_OLD}..upstream/master" || true

echo "== 3. 预判：上游是否动了我方注入文件 =="
git diff "${BASE_OLD}..upstream/master" -- \
  frame-app/src/app/chrome.rs \
  frame-app/src/app/mod.rs \
  frame-app/src/app/state.rs \
  frame-app/src/app/settings_actions.rs \
  frame-app/src/app/file_list_panel.rs \
  frame-app/src/file_queue/item.rs \
  frame-app/src/settings/model.rs \
  frame-app/src/assets/mod.rs \
  || true

OLD=$(git rev-parse "$BRANCH")
echo "== 4. 记录重放前栈顶: $OLD =="

echo "== 5. rebase 重放（冲突处停下，人工裁决；rerere 会自动重放已记录的解法）=="
if ! git rebase --onto upstream/master "$BASE_OLD" "$BRANCH"; then
  echo "!! 冲突：请人工裁决后 git add <files> && git rebase --continue；放弃则 git rebase --abort"
  exit 1
fi

echo "== 6. 对账：重放前后我方改动是否原样（逐段比对）=="
git range-diff "${BASE_OLD}..${OLD}" "upstream/master..${BRANCH}" || true

echo "== 7. 基线测试 =="
cargo test -p frame-app 2>&1 | tail -5

echo "== 8. 手测清单（人工）=="
cat <<'EOF'
  [ ] 标题栏：[应用(条件)][预设][设置][开始]；添加源已迁到文件列表表头 +
  [ ] 预设按钮文字：匹配预设名 / 自定义 / 预设 / 选择预设（多同型勾选时）
  [ ] 下拉：隐藏不兼容；自定义项在列首；当前项柔和高亮；＋新建；✏/✓ 编辑删除
  [ ] 应用按钮：勾选>=2 且同型才出现；有工作中文件则置灰；只作用勾选文件
  [ ] 查看预设后点"自定义"能恢复（Custom Work State 不丢）
  [ ] 空队列：欢迎页出现，表头 + 不出现（与原逻辑一致）
EOF

echo "== 完成。确认无误后人工执行: git push origin ${BRANCH} =="
