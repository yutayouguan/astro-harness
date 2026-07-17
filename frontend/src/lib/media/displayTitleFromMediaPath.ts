/** 从生成文件路径提炼展示标题（去掉时间戳-短 id 后缀）。 */
export function displayTitleFromMediaPath(path: string): string {
  const base = path.replace(/\\/g, "/").split("/").pop() ?? path;
  const stem = base.replace(/\.[^.]+$/, "");
  const cleaned = stem.replace(/-\d{8}-\d{6}-[a-f0-9]{8}$/i, "").trim();
  if (!cleaned) return base;
  // 旧前缀回落到中文种别感
  const legacy: Record<string, string> = {
    music: "音乐",
    img: "图片",
    vid: "视频",
    tts: "语音",
  };
  return legacy[cleaned.toLowerCase()] ?? cleaned;
}
