/**
 * 工作区 / 文件空间工具栏与分类图标 — 统一 lucide-react。
 * 文件条目图标请用 `lib/fileTypeIcon` 的 `resolveFileType`。
 */
import type { SVGProps } from "react";
import type { LucideIcon } from "lucide-react";
import {
  AlignJustify,
  ArrowLeft,
  BookText,
  ChevronUp,
  Database,
  File,
  FileCode2,
  FileImage,
  FileJson,
  FileLock2,
  FilePlus2,
  FileSpreadsheet,
  FileText,
  FileVideo,
  Folder,
  FolderPlus,
  Layers,
  LayoutGrid,
  List,
  MessageCircle,
  Presentation,
  Trash2,
} from "lucide-react";

type IconProps = SVGProps<SVGSVGElement> & {
  size?: number | string;
};

function iconSize(props: IconProps): number {
  const raw = props.size ?? props.width ?? props.height ?? 18;
  const n = typeof raw === "number" ? raw : Number.parseInt(String(raw), 10);
  return Number.isFinite(n) ? n : 18;
}

function WsLucide(Icon: LucideIcon, props: IconProps) {
  const { className, style, onClick } = props;
  return (
    <Icon
      size={iconSize(props)}
      strokeWidth={2}
      className={className}
      style={style}
      onClick={onClick}
      aria-hidden
    />
  );
}

/** 新建文件 */
export function IconWsNewFile(props: IconProps) {
  return WsLucide(FilePlus2, props);
}

/** 新建文件夹 */
export function IconWsNewFolder(props: IconProps) {
  return WsLucide(FolderPlus, props);
}

/** 返回对话 */
export function IconWsBackChat(props: IconProps) {
  return WsLucide(MessageCircle, props);
}

/** 上级目录 */
export function IconWsArrowUp(props: IconProps) {
  return WsLucide(ChevronUp, props);
}

/** 返回目录 */
export function IconWsArrowLeft(props: IconProps) {
  return WsLucide(ArrowLeft, props);
}

/** 删除 */
export function IconWsTrash(props: IconProps) {
  return WsLucide(Trash2, props);
}

/** 文件夹 */
export function IconWsFolder(props: IconProps) {
  return WsLucide(Folder, props);
}

/** 通用文件 */
export function IconWsFile(props: IconProps) {
  return WsLucide(File, props);
}

/** Markdown */
export function IconWsFileMd(props: IconProps) {
  return WsLucide(BookText, props);
}

/** JSON / 配置 */
export function IconWsFileJson(props: IconProps) {
  return WsLucide(FileJson, props);
}

/** 数据库相关 */
export function IconWsFileDb(props: IconProps) {
  return WsLucide(Database, props);
}

/** 加密 / 敏感 */
export function IconWsFileLock(props: IconProps) {
  return WsLucide(FileLock2, props);
}

/** 纯文本 */
export function IconWsFileText(props: IconProps) {
  return WsLucide(FileText, props);
}

/** 图片 */
export function IconWsFileImage(props: IconProps) {
  return WsLucide(FileImage, props);
}

/** 视频 */
export function IconWsFileVideo(props: IconProps) {
  return WsLucide(FileVideo, props);
}

/** 代码 */
export function IconWsFileCode(props: IconProps) {
  return WsLucide(FileCode2, props);
}

/** 表格 / 电子表格 */
export function IconWsFileSheet(props: IconProps) {
  return WsLucide(FileSpreadsheet, props);
}

/** 幻灯片 */
export function IconWsFileSlides(props: IconProps) {
  return WsLucide(Presentation, props);
}

/** 列表视图 */
export function IconWsViewList(props: IconProps) {
  return WsLucide(List, props);
}

/** 网格视图 */
export function IconWsViewGrid(props: IconProps) {
  return WsLucide(LayoutGrid, props);
}

/** 紧凑视图 */
export function IconWsViewCompact(props: IconProps) {
  return WsLucide(AlignJustify, props);
}

/** 图层 / 分组 */
export function IconWsLayers(props: IconProps) {
  return WsLucide(Layers, props);
}
