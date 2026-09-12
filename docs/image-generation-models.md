# 图片生成默认模型

OpenAI/Azure的自动策略按用户指定的两个部署选择，不通过提示词关键词猜测场景，也不宣称两者未经验证的速度、价格差异。

| 场景 | 自动模型 |
| --- | --- |
| 宠物/角色还原、动作补帧（`character` / `animation`） | `gpt-image-2.5-sunburst` |
| 壁纸、普通生图（`wallpaper` / `general`） | `gpt-image-2.5-flare` |
| 未声明场景但带参考图 | `gpt-image-2.5-sunburst` |
| 未声明场景且无参考图 | `gpt-image-2.5-flare` |

显式指定的模型/自定义部署名始终优先；Google、智谱等其它Provider保持各自默认值，不因为兼容OpenAI协议就切换成GPT图片模型。没有扩大Provider回退范围，也不在请求失败时偷偷换模型。

## 自动与固定选择

模型服务 → 对应OpenAI/Azure → 媒体模型 → 图片模型，选择“按场景自动”。持久化仍使用现有 `image_model`，空值表示自动；具体模型名表示固定选择。

读取DTO时保持自动字段为空，避免仅打开/保存设置就把它物化成固定Flare。Tauri目标、Agent媒体凭证和gRPC也保留空值，直到有请求场景/参考图后才解析。已保存的固定型号不自动清除。

2026-09-12检查本机配置：当前Azure仍固定为`gpt-image-2`，本轮未直接覆写正在运行的应用缓存或磁盘设置。原生设置界面连接未成功，因此实际切换自动仍待执行；代码策略完成不代表本机选择已改变。

## 链路

- `providers::image_gen::select_image_model`为统一选择器；`ImageScene`是明确场景类型。
- `image_gen`工具支持可选`purpose: character | animation | wallpaper | general`；未传使用参考图规则，工具输出记录实际使用的模型。
- 桌宠照片生成使用参考图规则；配套壁纸即使带宠物参考图也显式标记`Wallpaper`。
- Provider的普通生图默认、请求构建器、直接Images HTTP接口与设置页提示已统一为2.5策略。
- gRPC没有新增线字段：桌面已知场景时先解析型号；普通gRPC空型号请求在Provider根据参考图解析。

## 验证边界

Azure `/models`实查包含 `gpt-image-2.5-sunburst` / `gpt-image-2.5-flare`及其`2026-09-08`快照名。用户输入的`sunburs`和`fla`按这些已确认的完整部署名处理，不新增运行时别名。

本轮使用已授权的imagegen CLI、独立`openai==2.54.0`环境和Sunburst成功生成两组过渡姿态。未更改全局SDK、密钥、Provider端点或imagegen脚本。前几轮3.x环境旧模型请求断连，而本轮同时更换了模型和SDK，因此不能仅据此认定旧故障只由SDK引起。Flare只核对了部署列表，未做付费生图验证。
