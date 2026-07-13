# 辅模型 Smart 审批

**日期:** 2026-07-13  
**状态:** 已实现  
**前置:** [hitl-hardening](./2026-07-13-hitl-hardening-design.md)

## 目标

对规则分级为 `Ask` 的危险 terminal 命令，可选调用辅模型降级为 `Auto`；失败回退 `Ask`。

## 开关

- 默认**关闭**
- `ASTRO_SMART_APPROVAL=1` 开启

## 行为

1. `classify_dangerous_command` 不变（`Deny`/`Auto`/`Ask`）
2. 仅 `Ask` + 开关开启 → 短超时（8s）调当前会话 provider/model（无工具）
3. 模型回复解析：含 `AUTO`（词）→ 放行；否则 → `Ask`
4. 超时 / 错误 / 空回复 → `Ask`（HITL）
5. **永不**由辅模型升级为 `Deny` 或覆盖规则 `Deny`/`Auto`

## 非目标

独立小模型配置 UI；改白名单规则本身。
