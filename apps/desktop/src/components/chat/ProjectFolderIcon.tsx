import LucideByName from "../icons/LucideByName";
import {
  isMaterialProjectIcon,
  materialProjectIconUrl,
} from "../../lib/projects/materialProjectIcons";
import "./ProjectFolderIcon.css";

type Props = {
  iconId?: string | null;
  expanded: boolean;
  size?: number;
  className?: string;
};

export default function ProjectFolderIcon({
  iconId,
  expanded,
  size = 18,
  className,
}: Props) {
  if (iconId && !isMaterialProjectIcon(iconId)) {
    return <LucideByName name={iconId} size={size} className={className} />;
  }

  return (
    <img
      className={`project-folder-icon ${className ?? ""}`.trim()}
      src={materialProjectIconUrl(iconId, expanded)}
      width={size}
      height={size}
      alt=""
      draggable={false}
      aria-hidden
    />
  );
}
