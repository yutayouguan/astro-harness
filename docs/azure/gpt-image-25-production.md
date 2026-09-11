# GPT Image 2.5：接入与素材生产

更新：2026-09-11。

新增识别 `gpt-image-2.5-flare`、`gpt-image-2.5-sunburst`，以及两者的
`-2026-09-08` 快照。Azure 当前资源的 `/openai/v1/models` 已返回这两个别名及快照，
但模型列表不等同于任何 Azure 资源都已部署它们；应以实际生图成功为准。

## 接入

- 沿用 Azure/OpenAI Images 协议、已配置的 base URL 和凭证；不创建另一套 Provider。
- 设置 → 模型服务 → Azure → 刷新模型 → 媒体，可选择两个新模型。
- 服务端对已核实 ID 补上 `image_gen` 能力，即使 OpenRouter 尚未收录也不会从图片下拉列表消失；缓存再次 enrich 后仍保留。
- 前端离线能力推断同步支持；纯生图模型排除在自动聊天选模之外。
- 不合成未被 Provider 返回的可用部署，不更改现有全局 `image_model`、聊天模型或 API Key。
- Provider 原样传递显式模型 ID；不根据名字猜测质量等级，不静默换模型重试。

## 当前生产策略

两个模型先各制作一张真实需求中的场景，人工检查后继续同类场景。
本轮使用已授权的 `imagegen` 内置 CLI，经同一 Azure v1 资源：
`edit` + 一张画风参考图，`quality=medium`、`size=1536x1024`、
`output_format=jpeg`、`output_compression=100`。没有设置 `input_fidelity`。

Flare 制作森林小屋与雨天小窝；Sunburst 制作晴日草地与海边日落。
这是本批素材的人工分工，不是内置自动路由，也不是两个模型能力优劣的定论。
更高分辨率、原生透明、价格和稳定延迟尚未核实；不能因为名称带 Flare 就宣称更快或更便宜。
需要更高质量时针对不合格素材复审和迭代，不重复生成已验收的文件。

生成提示词、原图、来源与入库结果见
`output/imagegen/pet-scenes-20260911/PROGRESS.md`。参考图是现有内置宠物图，
只约束画风；壁纸中不重复画宠物。场景继承各自宠物默认设置，只入库，不自动应用。

## 验证边界

- 两个模型均已真实返回 JPEG；单次调用时长不能作为模型基准排名。
- 前端测试覆盖图片选项、保存值与自动聊天排除；Rust 测试覆盖无 OpenRouter 数据时的识别及请求参数。
- 元数据修改需运行新构建才生效；场景数据导入由现有 App 的状态文件监听刷新。
- 原生 UI 显示与交互验收独立于 CLI/API 成功，不把单元测试当成原生 App 验收。

本轮验证：前端定向测试29项、Rust模型元数据18项、图片HTTP契约9项通过；
`tsc --noEmit`、`npm run build`及`cargo check -p astro-agent`通过。
前端构建仍有既有的大chunk/混合动态导入提示。六张原图与入库副本哈希一致，
追加事务只改变scenes/revision，幂等重试不新增记录。
