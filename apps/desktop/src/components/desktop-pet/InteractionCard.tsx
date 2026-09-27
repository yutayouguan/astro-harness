import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowRight,
  Check,
  Clock3,
  Copy,
  Globe2,
  ShieldAlert,
  ShieldCheck,
  TerminalSquare,
  X,
} from "lucide-react";
import { parseApprovalContent } from "../../a2ui/clarifySteps";
import ClarifyWizard from "../../a2ui/ClarifyWizard";
import {
  approvalDetails,
  asRecord,
  inlineInteraction,
  simpleSchema,
  wizardSteps,
  redactInteractionDisplay,
  type PendingInteraction,
  type InteractionAction,
} from "../../lib/chat/pendingInteractions";
import {
  openInteractionSession,
  respondInteraction,
  usePendingInteractions,
} from "../../hooks/chat/usePendingInteractions";

type WizardDraft = {
  answers: Record<string, string>;
  customDrafts: Record<string, string>;
  index: number;
};
const drafts = new Map<
  string,
  { wizard?: WizardDraft; fields?: Record<string, unknown> }
>();
export default function InteractionCard({
  request,
  main = false,
}: {
  request: PendingInteraction;
  main?: boolean;
}) {
  const { connected, snapshot } = usePendingInteractions();
  const [busy, setBusy] = useState(false),
    lock = useRef(false);
  const [error, setError] = useState("");
  const [confirm, setConfirm] = useState<InteractionAction | null>(null);
  const [fields, setFields] = useState<Record<string, unknown>>(
    () => drafts.get(request.key)?.fields ?? {},
  );
  const expired =
    !!request.expiresAt && Date.parse(request.expiresAt) <= Date.now();
  const alive =
    snapshot.requests.some((r) => r.key === request.key) && !expired;
  const disabled = busy || !connected || !alive;
  const steps = wizardSteps(request),
    schema = simpleSchema(request);
  const approval = request.kind === "approval";
  // 网络类授权与工具批准共用一张卡，只是 eyebrow 与动作作用域不同；
  // 用挂起原因判定，不再靠动作 id 猜。
  const networkScoped = request.reason === "network_approval";
  const details = approval ? parseApprovalContent(approvalDetails(request)) : null;
  const primaryAction = request.actions.find(
    (action) => !action.persistent && action.id !== "deny",
  );
  const denyAction = request.actions.find((action) => action.id === "deny");
  const scopedActions = request.actions.filter((action) => action.persistent);
  const scopedIcon = (id: string) =>
    id === "allow_session" ? (
      <Clock3 size={16} strokeWidth={2} aria-hidden />
    ) : id === "approve_type" ? (
      <TerminalSquare size={16} strokeWidth={2} aria-hidden />
    ) : id === "allow_always" ? (
      <ShieldCheck size={16} strokeWidth={2} aria-hidden />
    ) : (
      <ShieldCheck size={16} strokeWidth={2} aria-hidden />
    );
  const [detailsCopied, setDetailsCopied] = useState(false);
  const copyDetails = () => {
    const text = details?.command ?? approvalDetails(request);
    void navigator.clipboard
      ?.writeText(text)
      .then(() => {
        setDetailsCopied(true);
        window.setTimeout(() => setDetailsCopied(false), 1600);
      })
      .catch(() => undefined);
  };
  const task = snapshot.tasks.find((t) => t.sessionId === request.sessionId);
  const saveDraft = useCallback(
    (wizard: WizardDraft) => {
      drafts.set(request.key, { ...drafts.get(request.key), wizard });
    },
    [request.key],
  );
  useEffect(() => {
    drafts.set(request.key, { ...drafts.get(request.key), fields });
  }, [fields, request.key]);
  useEffect(() => {
    for (const key of drafts.keys())
      if (connected && !snapshot.requests.some((r) => r.key === key))
        drafts.delete(key);
  }, [connected, snapshot]);
  async function submit(
    action: string,
    payload: Record<string, unknown>,
    confirmed = false,
  ) {
    if (disabled || lock.current) return;
    lock.current = true;
    setBusy(true);
    setError("");
    try {
      await respondInteraction(request, action, payload, confirmed);
      drafts.delete(request.key);
      setConfirm(null);
    } catch (e) {
      setError(String(e));
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  return (
    <section
      className="pet-interaction-card"
      data-pending-interaction-key={request.key}
      aria-busy={busy}
    >
      <header className="pet-interaction-header">
        <span className="pet-interaction-mark" aria-hidden>
          {request.kind !== "approval" ? (
            <Check size={18} strokeWidth={2.2} />
          ) : networkScoped ? (
            <Globe2 size={18} strokeWidth={2} />
          ) : (
            <ShieldAlert size={18} strokeWidth={2} />
          )}
        </span>
        <div className="pet-interaction-heading">
          <small className="pet-interaction-eyebrow">
            {approval
              ? networkScoped
                ? "需要授权"
                : "需要批准"
              : "需要你的回答"}
          </small>
          <h3>
            {approval
              ? "确认待执行操作"
              : redactInteractionDisplay(request.message)}
          </h3>
          <small className="pet-task-project">
            {[task?.title ?? "任务", task?.project]
              .filter(Boolean)
              .join(" · ")}
          </small>
        </div>
      </header>
      {approval && details ? (
        <details open className="pet-interaction-details-block">
          <summary>
            <span>操作说明与权限范围</span>
            <button
              type="button"
              className="pet-interaction-copy"
              onClick={copyDetails}
              aria-label="复制命令"
              title="复制命令"
            >
              {detailsCopied ? (
                <Check size={13} strokeWidth={2.4} aria-hidden />
              ) : (
                <Copy size={13} strokeWidth={2} aria-hidden />
              )}
            </button>
          </summary>
          {details.description ? (
            <p className="pet-interaction-description">{details.description}</p>
          ) : null}
          {details.command ? (
            <pre className="pet-interaction-details">
              <code>{details.command}</code>
            </pre>
          ) : null}
        </details>
      ) : null}
      {!alive && <p role="status">此请求已处理、取消或过期。</p>}
      {!connected && (
        <p role="status">连接中断，恢复后才能提交。输入已保留。</p>
      )}
      {error && (
        <p role="alert" className="desktop-pet-error">
          {error}
        </p>
      )}
      {!inlineInteraction(request) ? (
        <p>此请求需要完整会话或外部授权页面，请到会话处理。</p>
      ) : request.kind === "approval" ? (
        confirm ? (
          <div
            className="pet-persistent-confirm"
            role="group"
            aria-label="确认长期授权"
          >
            <strong>{confirm.label}</strong>
            <p>
              {confirm.id === "allow_session"
                ? "此授权持续到本会话结束。"
                : "此授权会持续生效，后续匹配的操作可能不再询问。"}
              请核对上方操作及权限范围。
            </p>
            <button
              className={`pet-interaction-action is-scoped is-${confirm.id}`}
              type="button"
              disabled={disabled}
              onClick={() => void submit(confirm.id, {}, true)}
            >
              {scopedIcon(confirm.id)}
              <span>确认{confirm.label}</span>
            </button>
            <button
              className="pet-task-quiet"
              type="button"
              disabled={busy}
              onClick={() => setConfirm(null)}
            >
              返回
            </button>
          </div>
        ) : (
          <div className="pet-interaction-actions">
            {denyAction ? (
              <button
                key={denyAction.id}
                className="pet-interaction-action is-deny"
                type="button"
                disabled={disabled}
                onClick={() => void submit(denyAction.id, {})}
              >
                <X size={16} strokeWidth={2.2} aria-hidden />
                <span>{denyAction.label}</span>
              </button>
            ) : null}
            {primaryAction ? (
              <button
                key={primaryAction.id}
                className="pet-interaction-action is-primary"
                type="button"
                disabled={disabled}
                onClick={() => void submit(primaryAction.id, {})}
              >
                <Check size={16} strokeWidth={2.3} aria-hidden />
                <span>{primaryAction.label}</span>
              </button>
            ) : null}
            {scopedActions.length > 0 ? (
              <div className="pet-interaction-scoped">
                {scopedActions.map((action) => (
                  <button
                    key={action.id}
                    className={`pet-interaction-action is-scoped is-${action.id}`}
                    type="button"
                    disabled={disabled}
                    onClick={() => setConfirm(action)}
                  >
                    {scopedIcon(action.id)}
                    <span>{action.label}</span>
                    <ArrowRight
                      className="pet-interaction-arrow"
                      size={15}
                      aria-hidden
                    />
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        )
      ) : steps.length ? (
        <ClarifyWizard
          steps={steps}
          disabled={disabled}
          deferCommit
          initialDraft={drafts.get(request.key)?.wizard}
          onDraftChange={saveDraft}
          onAction={(_name, context) => void submit("submit", context)}
        />
      ) : schema ? (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void submit("submit", fields);
          }}
        >
          {Object.entries(schema.properties).map(([name, raw]) => {
            const field = asRecord(raw),
              value = fields[name];
            return (
              <label key={name}>
                <span>
                  {String(field.title ?? field.description ?? name)}
                  {schema.required.has(name) ? " *" : ""}
                </span>
                {Array.isArray(field.enum) ? (
                  <select
                    disabled={disabled}
                    required={schema.required.has(name)}
                    value={
                      value === undefined
                        ? ""
                        : String(field.enum.indexOf(value))
                    }
                    onChange={(e) => {
                      const next = { ...fields };
                      if (e.currentTarget.value === "") delete next[name];
                      else
                        next[name] = (field.enum as unknown[])[
                          Number(e.currentTarget.value)
                        ];
                      setFields(next);
                    }}
                  >
                    <option value="">请选择</option>
                    {field.enum.map((item, i) => (
                      <option key={i} value={String(i)}>
                        {String(item)}
                      </option>
                    ))}
                  </select>
                ) : field.type === "boolean" ? (
                  <select
                    disabled={disabled}
                    required={schema.required.has(name)}
                    value={value === undefined ? "" : String(value)}
                    onChange={(e) => {
                      const next = { ...fields };
                      if (e.currentTarget.value === "") delete next[name];
                      else next[name] = e.currentTarget.value === "true";
                      setFields(next);
                    }}
                  >
                    <option value="">请选择</option>
                    <option value="true">是</option>
                    <option value="false">否</option>
                  </select>
                ) : (
                  <input
                    disabled={disabled}
                    required={schema.required.has(name)}
                    type={field.type === "string" ? "text" : "number"}
                    minLength={
                      typeof field.minLength === "number"
                        ? field.minLength
                        : undefined
                    }
                    maxLength={
                      typeof field.maxLength === "number"
                        ? field.maxLength
                        : undefined
                    }
                    min={
                      typeof field.minimum === "number"
                        ? field.minimum
                        : undefined
                    }
                    max={
                      typeof field.maximum === "number"
                        ? field.maximum
                        : undefined
                    }
                    step={field.type === "integer" ? 1 : "any"}
                    value={value === undefined ? "" : String(value)}
                    onChange={(e) => {
                      const next = { ...fields };
                      if (!e.currentTarget.value) delete next[name];
                      else
                        next[name] =
                          field.type === "string"
                            ? e.currentTarget.value
                            : Number(e.currentTarget.value);
                      setFields(next);
                    }}
                  />
                )}
              </label>
            );
          })}
          <button
            className="pet-task-primary"
            type="submit"
            disabled={disabled}
          >
            提交回答
          </button>
        </form>
      ) : null}
      {request.serverName && inlineInteraction(request) && (
        <button
          type="button"
          disabled={disabled}
          onClick={() => void submit("decline", {})}
        >
          拒绝提供信息
        </button>
      )}
      {!main && (
        <button
          type="button"
          className="pet-task-link"
          onClick={() =>
            void openInteractionSession(request).catch((e) =>
              setError(String(e)),
            )
          }
        >
          查看会话
        </button>
      )}
    </section>
  );
}
