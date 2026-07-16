/** 空状态插画壳：场景图 + 可选标题副文案。 */
import type { HTMLAttributes, ReactNode } from "react";
import { EMPTY_SCENE_ART, type EmptyScene } from "./registry";

type Props = {
  scene: EmptyScene;
  title?: string;
  hint?: string;
  children?: ReactNode;
  className?: string;
  size?: "sm" | "md" | "lg";
  role?: HTMLAttributes<HTMLDivElement>["role"];
};

export default function EmptyIllustration({
  scene,
  title,
  hint,
  children,
  className = "",
  size = "md",
  role,
}: Props) {
  const Art = EMPTY_SCENE_ART[scene];
  return (
    <div
      className={`astro-empty astro-empty--${size} ${className}`.trim()}
      data-scene={scene}
      role={role}
    >
      <div className="astro-empty-art" aria-hidden>
        <Art />
      </div>
      {title ? <p className="astro-empty-title">{title}</p> : null}
      {hint ? <p className="astro-empty-hint">{hint}</p> : null}
      {children}
    </div>
  );
}
