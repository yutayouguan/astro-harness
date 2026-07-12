/** 轻量 Toast 提示。 */
/** Toast 入参 */
type Props = {
  message: string;
  visible: boolean;
};

export function Toast({ message, visible }: Props) {
  if (!visible || !message) return null;
  return (
    <div className="astro-toast" role="status" aria-live="polite">
      {message}
    </div>
  );
}
