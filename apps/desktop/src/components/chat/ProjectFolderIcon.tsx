import LucideByName from "../icons/LucideByName";
import {
  isBrandProjectIcon,
  isDefaultProjectIcon,
  isMaterialProjectIcon,
  materialProjectIconUrl,
} from "../../lib/projects/materialProjectIcons";
import ProjectFolderGlyph from "./ProjectFolderGlyph";
import "./ProjectFolderIcon.css";

type Props = {
  iconId?: string | null;
  expanded: boolean;
  size?: number;
  className?: string;
  loading?: "eager" | "lazy";
};

export default function ProjectFolderIcon({
  iconId,
  expanded,
  size = 18,
  className,
  loading,
}: Props) {
  if (iconId && !isMaterialProjectIcon(iconId)) {
    return <LucideByName name={iconId} size={size} className={className} />;
  }

  const classes = `project-folder-icon ${className ?? ""}`.trim();
  const brand = isBrandProjectIcon(iconId);

  // 默认文件夹和品牌图标内联渲染，填色才能跟着 --tone 走。
  if (brand || isDefaultProjectIcon(iconId)) {
    return (
      <ProjectFolderGlyph
        expanded={expanded}
        brand={brand}
        size={size}
        className={`${classes} project-folder-icon--tinted`}
      />
    );
  }

  return (
    <img
      className={classes}
      src={materialProjectIconUrl(iconId, expanded)}
      width={size}
      height={size}
      alt=""
      loading={loading}
      draggable={false}
      aria-hidden
    />
  );
}
