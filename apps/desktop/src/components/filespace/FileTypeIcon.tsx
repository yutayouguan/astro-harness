/** 彩色文件类型图标（material-icon-theme 资源，按文件名/扩展名/文件夹名解析）。 */
import { materialIconUrl } from "../../lib/filespace/materialFileIcons";

type Props = {
  name: string;
  isDir?: boolean;
  /** 目录展开时使用打开态图标 */
  expanded?: boolean;
  size?: number;
  className?: string;
};

export default function FileTypeIcon({
  name,
  isDir = false,
  expanded = false,
  size = 16,
  className = "file-type-icon",
}: Props) {
  return (
    <img
      className={className}
      src={materialIconUrl(name, { isDir, expanded })}
      width={size}
      height={size}
      alt=""
      aria-hidden
      draggable={false}
    />
  );
}
