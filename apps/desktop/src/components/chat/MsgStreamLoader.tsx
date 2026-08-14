/** 整轮流式生成期间常驻的点阵加载指示。 */
import { useI18n } from "../../i18n/LocaleContext";

const CELLS = 9;

type Props = {
  /** 无正文时作为主占位，有正文时贴在底部 */
  alone?: boolean;
};

export default function MsgStreamLoader({ alone = false }: Props) {
  const { t } = useI18n();
  return (
    <div
      className={`msg-stream-loader${alone ? " is-alone" : ""}`}
      aria-label={t("chat.generating")}
      role="status"
    >
      {Array.from({ length: CELLS }, (_, i) => (
        <span
          key={i}
          style={{ animationDelay: `${(i % 3) * 0.12 + Math.floor(i / 3) * 0.08}s` }}
        />
      ))}
    </div>
  );
}
