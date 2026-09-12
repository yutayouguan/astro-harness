import { useCallback, useEffect, useRef, useState } from "react";
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
      <header>
        <small className="pet-interaction-eyebrow">
          {request.kind === "approval" ? "需要审批" : "需要你的回答"} ·{" "}
          {task?.title ?? "任务"}
        </small>
        {task?.project && (
          <small className="pet-task-project">{task.project}</small>
        )}
        <h3>
          {request.kind === "approval"
            ? "确认待执行操作"
            : redactInteractionDisplay(request.message)}
        </h3>
      </header>
      {request.kind === "approval" && (
        <details open>
          <summary>操作说明与权限范围</summary>
          <pre className="pet-interaction-details">
            {approvalDetails(request)}
          </pre>
        </details>
      )}
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
              className="pet-task-primary"
              type="button"
              disabled={disabled}
              onClick={() => void submit(confirm.id, {}, true)}
            >
              确认{confirm.label}
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => setConfirm(null)}
            >
              返回
            </button>
          </div>
        ) : (
          <div className="pet-interaction-actions">
            {request.actions.map((action) => (
              <button
                key={action.id}
                className={
                  action.id === "approve" || action.id === "allow_once"
                    ? "pet-task-primary"
                    : ""
                }
                type="button"
                disabled={disabled}
                onClick={() =>
                  action.persistent
                    ? setConfirm(action)
                    : void submit(action.id, {})
                }
              >
                {action.label}
              </button>
            ))}
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
