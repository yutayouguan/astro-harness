/** 主题色驱动的项目文件夹字形：默认文件夹与品牌主空间图标都随当前 Tab 变色。 */
import { useId } from "react";
import { ASTRO_MARK_PATH } from "../icons/AstroLogoMark";

/** 与 material-icon-theme 的 folder / folder-open 同形，只是填色交给 CSS。 */
const FOLDER_CLOSED_PATH =
  "m6.922 3.768-.644-.536A1 1 0 0 0 5.638 3H2a1 1 0 0 0-1 1v8a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V5a1 1 0 0 0-1-1H7.562a1 1 0 0 1-.64-.232";
const FOLDER_OPEN_PATH =
  "M14.483 6H4.721a1 1 0 0 0-.949.684L2 12V5h12a1 1 0 0 0-1-1H7.562a1 1 0 0 1-.64-.232l-.644-.536A1 1 0 0 0 5.638 3H2a1 1 0 0 0-1 1v8a1 1 0 0 0 1 1h11l2.403-5.606A1 1 0 0 0 14.483 6";

type Props = {
  /** 展开态用敞口文件夹。 */
  expanded: boolean;
  /** 品牌主空间：渐变填色 + 白色主标；否则单色文件夹。 */
  brand: boolean;
  size: number;
  className?: string;
};

export default function ProjectFolderGlyph({
  expanded,
  brand,
  size,
  className,
}: Props) {
  const gradId = `projectFolderGrad-${useId().replace(/:/g, "")}`;

  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 16 16"
      width={size}
      height={size}
      className={className}
      aria-hidden
    >
      {brand && (
        <defs>
          <linearGradient
            id={gradId}
            gradientUnits="userSpaceOnUse"
            x1="1.5"
            y1="13.5"
            x2="14.5"
            y2="3"
          >
            <stop
              offset="0%"
              style={{ stopColor: "var(--astro-mark-c0, #0084fd)" }}
            />
            <stop
              offset="46%"
              style={{ stopColor: "var(--astro-mark-c1, #1d57fd)" }}
            />
            <stop
              offset="100%"
              style={{ stopColor: "var(--astro-mark-c2, #6020fc)" }}
            />
          </linearGradient>
        </defs>
      )}
      <path
        d={expanded ? FOLDER_OPEN_PATH : FOLDER_CLOSED_PATH}
        fill={brand ? `url(#${gradId})` : "currentColor"}
      />
      {brand && (
        <path
          d={ASTRO_MARK_PATH}
          fill="#fff"
          fillOpacity={0.94}
          transform="translate(5.45 6.25) scale(.0155) translate(-205 -127)"
        />
      )}
    </svg>
  );
}
