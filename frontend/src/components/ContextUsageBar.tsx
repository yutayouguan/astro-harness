/** 上下文占用分段条：按 visibleSegments 比例着色，剩余容量为 muted rest。 */
import {
  SEGMENT_TONE,
  visibleSegments,
  type ContextUsageSnapshot,
} from "../lib/contextUsage";

type Props = {
  snapshot: ContextUsageSnapshot;
  windowTokens: number;
};

export default function ContextUsageBar({ snapshot, windowTokens }: Props) {
  const segs = visibleSegments(snapshot);
  const win = windowTokens > 0 ? windowTokens : 1;
  return (
    <div className="ctx-usage-bar" role="img" aria-hidden>
      {segs.map((s) => (
        <span
          key={s.id}
          className="ctx-usage-bar-seg"
          style={{
            flexGrow: s.tokens,
            background: `var(${SEGMENT_TONE[s.id] ?? "--accent"})`,
            maxWidth: `${(s.tokens / win) * 100}%`,
          }}
        />
      ))}
      <span
        className="ctx-usage-bar-rest"
        style={{ flexGrow: Math.max(0, win - snapshot.totalTokens) }}
      />
    </div>
  );
}
