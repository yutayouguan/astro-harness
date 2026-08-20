# WASM 插件沙箱（暂缓）

> 状态：暂缓 | 原因：MCP 协议 + SKILL.md 脚本已覆盖扩展需求
> 原文档：523 行详细设计（wasmtime 运行时、Host Functions、Plugin SDK），已归档精简

## 暂缓理由

1. MCP（Model Context Protocol）已成为行业标准的工具扩展协议，Astro 采用 STDIO/Streamable HTTP 传输
2. SKILL.md 脚本系统支持 Python/JS 自定义逻辑，覆盖大部分自动化场景
3. WASM 插件引入第三方扩展机制（wasmtime 运行时、Host Functions、Plugin SDK），与上述两者三重覆盖
4. 开发 WASM SDK（Rust + TypeScript）需 5-8 工程周，在零第三方开发者阶段投入产出比低

## 何时重新激活

- 当 MCP 无法满足的需求出现（如：需要高性能纯计算、需要严格内存隔离的安全场景）
- 当第三方开发者生态形成，需要比 SKILL.md 更强的沙箱隔离

## 替代方案

| 需求 | 替代方案 |
|------|---------|
| 工具扩展 | MCP Server |
| 自定义逻辑 | SKILL.md + Python/JS 脚本 |
| 沙箱执行 | Docker 后端（v0.2 已支持） |
| 数据转换 | Rhai 脚本引擎（内置） |
