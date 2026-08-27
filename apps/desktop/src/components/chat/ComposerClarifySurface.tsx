import A2UIRenderer from "../../a2ui/A2UIRenderer";
import type { UiSurface } from "../../types";

type Props = {
  surface: UiSurface;
  mediaBaseDir?: string | null;
  onAction: (name: string, context: Record<string, unknown>) => void;
};

export default function ComposerClarifySurface({
  surface,
  mediaBaseDir,
  onAction,
}: Props) {
  return (
    <div className="composer-clarify-surface" aria-live="polite">
      <A2UIRenderer
        operations={surface.operations}
        mediaBaseDir={mediaBaseDir}
        onAction={onAction}
      />
    </div>
  );
}
