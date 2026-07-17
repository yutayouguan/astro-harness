/** 文件类型图标：按后缀映射 Lucide 图标并按种类着色（工作空间 / 文件空间共用）。 */
import { resolveFileType } from "../../lib/filespace/fileTypeIcon";

type Props = {
  name: string;
  isDir?: boolean;
  /** 着色容器类名：工作空间用 ws-file-glyph，文件空间用 fs-file-glyph */
  className?: string;
  size?: number;
};

export default function FileGlyph({
  name,
  isDir = false,
  className = "ws-file-glyph",
  size = 18,
}: Props) {
  const { kind, Icon } = resolveFileType(name, isDir);
  return (
    <span className={className} data-kind={kind} aria-hidden>
      <Icon size={size} strokeWidth={2} />
    </span>
  );
}
