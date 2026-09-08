import { useId } from "react";
import { RotateCw, Settings2, ShieldAlert } from "lucide-react";
import { Button } from "../ui/Button";
import type { ConnectionIssueKind } from "../../lib/ui/onboarding";
const COPY = {
  zh: {
    verification: [
      "需要重新验证模型连接",
      "配置发生变化、验证已过期或应用已重启，请重新测试连接后继续。",
    ],
    credentials: [
      "密钥或访问权限有问题",
      "检查密钥是否属于当前服务、是否已启用该模型，修改后重试。",
    ],
    quota: [
      "账户余额或额度不足",
      "前往服务商控制台检查余额和项目配额，或切换到其他模型服务。",
    ],
    model: [
      "模型或部署不存在",
      "核对模型名称、部署名称与服务地址；它们必须属于同一个模型服务。",
    ],
    timeout: [
      "连接测试超时",
      "检查网络和服务地址后重试。连接测试通过后才能继续初始化。",
    ],
    network: [
      "暂时无法连接服务",
      "检查网络、代理与服务地址，恢复后重试。不能跳过连接验证。",
    ],
    rate_limit: [
      "请求频率受限",
      "稍等片刻再测试，或切换服务商/项目以使用其他配额。",
    ],
    unknown: [
      "连接没有通过验证",
      "检查服务地址、密钥与模型配置后重试，验证通过后才能继续。",
    ],
  },
  en: {
    verification: [
      "Verify the model connection again",
      "Configuration changed, verification expired, or the app restarted. Test the connection again to continue.",
    ],
    credentials: [
      "Key or access problem",
      "Check the key's provider and model access, then edit the key and retry.",
    ],
    quota: [
      "Insufficient credits or quota",
      "Check billing and project quota in the provider console, or choose another provider.",
    ],
    model: [
      "Model or deployment not found",
      "Check that the model/deployment name and endpoint belong to the same provider.",
    ],
    timeout: [
      "Connection test timed out",
      "Check your network and endpoint, then retry. A successful connection test is required.",
    ],
    network: [
      "Service is unreachable",
      "Check the network, proxy and endpoint. Verification is required before continuing.",
    ],
    rate_limit: [
      "Rate limit reached",
      "Wait before retrying, or choose a provider/project with available quota.",
    ],
    unknown: [
      "Connection could not be verified",
      "Check the endpoint, key and model, then retry. Verification is required.",
    ],
  },
} as const;
export function ConnectionIssue({
  kind,
  locale,
  onRetry,
  onEdit,
}: {
  kind: ConnectionIssueKind;
  locale: "zh" | "en";
  onRetry: () => void;
  onEdit?: () => void;
}) {
  const [title, hint] = COPY[locale][kind];
  const titleId = useId();
  const hintId = useId();
  return (
    <div
      className="onboarding-connection-issue"
      role="alert"
      data-issue={kind}
      aria-labelledby={titleId}
      aria-describedby={hintId}
    >
      <span className="onboarding-connection-issue__icon" aria-hidden="true">
        <ShieldAlert size={18} strokeWidth={1.8} />
      </span>
      <div className="onboarding-connection-issue__body">
        <strong id={titleId} className="onboarding-connection-issue__title">
          {title}
        </strong>
        <p id={hintId} className="onboarding-connection-issue__hint">
          {hint}
        </p>
        <div className="onboarding-connection-issue__actions">
          {onEdit && (
            <Button variant="secondary" onClick={onEdit}>
              <Settings2 size={14} aria-hidden="true" />
              {locale === "zh" ? "修改配置" : "Edit configuration"}
            </Button>
          )}
          <Button variant="primary" onClick={onRetry}>
            <RotateCw size={14} aria-hidden="true" />
            {locale === "zh" ? "重试连接" : "Retry connection"}
          </Button>
        </div>
      </div>
    </div>
  );
}
