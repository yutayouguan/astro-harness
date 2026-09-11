# 本地宠物场景批量导入

把已生成且验收通过的壁纸关联到已有宠物；不生成图片、不应用场景、不切换桌面。
使用与App相同的`types::update_desktop_pet_state`共享锁与原子替换，不手工编辑运行中的state.json。

```sh
cargo run -p tools --example import_pet_scenes -- /absolute/astro-home /absolute/scene-pack/scenes.json
cargo run -p tools --example import_pet_scenes -- /absolute/astro-home /absolute/scene-pack/scenes.json --apply
```

第一条预览；只有显式`--apply`才导入。目标必须已存在宠物库。

清单格式：

```json
{
  "expectedSceneIds": [],
  "scenes": [{
    "id": "pet-unique-scene-id",
    "petId": "existing-pet-id",
    "name": "午后窗台",
    "wallpaper": "images/afternoon.jpg",
    "recommendedTheme": "light"
  }]
}
```

- `expectedSceneIds`是这些目标宠物在生成前已有的场景ID，用于核对生成期间的增删；不覆盖已有条目。
- 壁纸须在清单目录内；拒绝相对路径逃逸和越界符号链接，校验文件大小、格式与解码尺寸。
- 先准备完整图片副本，再单事务追加整批场景；使用事务内最新宠物身份。失败不写入半批场景。
- 场景继承宠物默认配置；当前显示、壁纸、大小、位置、暂停等字段保持不变。
- 整批ID已存在且绑定图片一致时不重复导入；冲突、部分已存在或已改绑时拒绝覆盖。
- App的宠物状态文件监听会刷新场景库，无需为数据导入修改/重建App。

测试：`cargo test -p tools --example import_pet_scenes`。覆盖默认预览、原子追加、两只宠物各3场景、幂等、路径逃逸、缺图/缺宠物、并发场景变化和部分重导入。

2026-09-11生成任务已完成：奶糖（午后窗台、月光书房、森林小屋）与布丁（晴日草地、海边日落、雨天小窝）各3个真实场景已入库。
其中4张由用户指定的新模型Flare/Sunburst制作，原有2张保留。来源、提示词、哈希与验收记录位于
`output/imagegen/pet-scenes-20260911/PROGRESS.md`和`source-and-validation.json`。
追加场景不改变用户已选的奶糖/午后窗台；原生UI界面验收仍独立待确认。
