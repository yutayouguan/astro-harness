/** 命令式确认 / 输入对话框：useConfirm / usePrompt。 */
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import AppDialog from "../../components/ui/AppDialog";
import { useI18n } from "../../i18n/LocaleContext";

export type ConfirmOptions = {
  title: string;
  message: string;
  /** 醒目展示的对象名（如待删会话标题），渲染在正文上方。 */
  emphasis?: string;
  /** emphasis 上方的短标签，例如「会话」。 */
  emphasisLabel?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  variant?: "default" | "danger";
  /** 附加勾选项；用 confirmDetailed 读取勾选结果，confirm 仍只返回是否确认。 */
  options?: ConfirmOptionItem[];
};

/** 确认弹窗里的附加勾选项。 */
export type ConfirmOptionItem = {
  id: string;
  label: string;
  description?: string;
  /** 默认勾选状态（缺省为未勾选）。 */
  defaultChecked?: boolean;
  /** 危险选项：渲染为警示样式，通常配合 defaultChecked 为 false。 */
  danger?: boolean;
};

/** 确认结果；未声明 options 时 `selected` 为空数组。 */
export type ConfirmOutcome = {
  confirmed: boolean;
  selected: string[];
};

export type PromptOptions = {
  title: string;
  message?: string;
  defaultValue?: string;
  placeholder?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  /** 返回错误文案则阻止确认；空串视为无效。 */
  allowEmpty?: boolean;
};

type ConfirmRequest = {
  kind: "confirm";
  options: ConfirmOptions;
  resolve: (outcome: ConfirmOutcome) => void;
};

type PromptRequest = {
  kind: "prompt";
  options: PromptOptions;
  resolve: (value: string | null) => void;
};

type DialogRequest = ConfirmRequest | PromptRequest;

type DialogApi = {
  confirm: (options: ConfirmOptions) => Promise<boolean>;
  /** 带勾选项的确认：返回确认结果与勾选项 id 列表。 */
  confirmDetailed: (options: ConfirmOptions) => Promise<ConfirmOutcome>;
  prompt: (options: PromptOptions) => Promise<string | null>;
};

const DialogContext = createContext<DialogApi | null>(null);

export function DialogProvider({ children }: { children: ReactNode }) {
  const { t } = useI18n();
  const [request, setRequest] = useState<DialogRequest | null>(null);
  const [draft, setDraft] = useState("");
  const [checked, setChecked] = useState<Record<string, boolean>>({});
  const queueRef = useRef<DialogRequest[]>([]);
  const activeRef = useRef<DialogRequest | null>(null);

  const pump = useCallback(() => {
    if (activeRef.current) return;
    const next = queueRef.current.shift() ?? null;
    activeRef.current = next;
    if (!next) {
      setRequest(null);
      setDraft("");
      setChecked({});
      return;
    }
    if (next.kind === "prompt") {
      setDraft(next.options.defaultValue ?? "");
      setChecked({});
    } else {
      setDraft("");
      const initial: Record<string, boolean> = {};
      for (const option of next.options.options ?? []) {
        initial[option.id] = option.defaultChecked ?? false;
      }
      setChecked(initial);
    }
    setRequest(next);
  }, []);

  const enqueue = useCallback(
    (item: DialogRequest) => {
      queueRef.current.push(item);
      pump();
    },
    [pump],
  );

  const closeActive = useCallback(
    (settle: () => void) => {
      settle();
      activeRef.current = null;
      setRequest(null);
      setDraft("");
      setChecked({});
      // 下一帧再弹出队列，避免同一次点击连环触发
      window.setTimeout(() => pump(), 0);
    },
    [pump],
  );

  const confirmDetailed = useCallback(
    (options: ConfirmOptions) =>
      new Promise<ConfirmOutcome>((resolve) => {
        enqueue({ kind: "confirm", options, resolve });
      }),
    [enqueue],
  );

  const confirm = useCallback(
    (options: ConfirmOptions) =>
      confirmDetailed(options).then((outcome) => outcome.confirmed),
    [confirmDetailed],
  );

  const prompt = useCallback(
    (options: PromptOptions) =>
      new Promise<string | null>((resolve) => {
        enqueue({ kind: "prompt", options, resolve });
      }),
    [enqueue],
  );

  const api = useMemo(
    () => ({ confirm, confirmDetailed, prompt }),
    [confirm, confirmDetailed, prompt],
  );

  const cancelLabel = request?.options.cancelLabel ?? t("dialog.cancel");
  const confirmLabel =
    request?.kind === "prompt"
      ? (request.options.confirmLabel ?? t("dialog.save"))
      : (request?.options.confirmLabel ?? t("dialog.confirm"));

  const promptEmpty =
    request?.kind === "prompt" && !request.options.allowEmpty && !draft.trim();

  return (
    <DialogContext.Provider value={api}>
      {children}
      {request ? (
        <AppDialog
          open
          title={request.options.title}
          message={
            request.kind === "confirm"
              ? request.options.message
              : request.options.message
          }
          emphasis={
            request.kind === "confirm" ? request.options.emphasis : undefined
          }
          emphasisLabel={
            request.kind === "confirm"
              ? request.options.emphasisLabel
              : undefined
          }
          variant={
            request.kind === "prompt"
              ? "prompt"
              : (request.options.variant ?? "default")
          }
          confirmLabel={confirmLabel}
          cancelLabel={cancelLabel}
          confirmDisabled={promptEmpty}
          onCancel={() => {
            if (request.kind === "confirm") {
              closeActive(() =>
                request.resolve({ confirmed: false, selected: [] }),
              );
            } else {
              closeActive(() => request.resolve(null));
            }
          }}
          onConfirm={() => {
            if (request.kind === "confirm") {
              const selected = (request.options.options ?? [])
                .filter((option) => checked[option.id])
                .map((option) => option.id);
              closeActive(() => request.resolve({ confirmed: true, selected }));
              return;
            }
            const value = draft.trim();
            if (!request.options.allowEmpty && !value) return;
            closeActive(() => request.resolve(value));
          }}
        >
          {request.kind === "prompt" ? (
            <input
              className="app-dialog-input"
              value={draft}
              placeholder={request.options.placeholder}
              onChange={(e) => setDraft(e.target.value)}
            />
          ) : request.options.options?.length ? (
            <div className="app-dialog-options">
              {request.options.options.map((option) => (
                <label
                  key={option.id}
                  className={`app-dialog-option ${option.danger ? "is-danger" : ""}`}
                >
                  <input
                    type="checkbox"
                    data-dialog-skip-autofocus
                    checked={checked[option.id] ?? false}
                    onChange={(e) =>
                      setChecked((prev) => ({
                        ...prev,
                        [option.id]: e.target.checked,
                      }))
                    }
                  />
                  <span className="app-dialog-option-text">
                    <span className="app-dialog-option-label">
                      {option.label}
                    </span>
                    {option.description ? (
                      <span className="app-dialog-option-desc">
                        {option.description}
                      </span>
                    ) : null}
                  </span>
                </label>
              ))}
            </div>
          ) : null}
        </AppDialog>
      ) : null}
    </DialogContext.Provider>
  );
}

export function useConfirm(): DialogApi["confirm"] {
  const ctx = useContext(DialogContext);
  if (!ctx) {
    throw new Error("useConfirm must be used within DialogProvider");
  }
  return ctx.confirm;
}

/** 带勾选项的确认；返回确认结果与勾选项 id 列表。 */
export function useConfirmDetailed(): DialogApi["confirmDetailed"] {
  const ctx = useContext(DialogContext);
  if (!ctx) {
    throw new Error("useConfirmDetailed must be used within DialogProvider");
  }
  return ctx.confirmDetailed;
}

export function usePrompt(): DialogApi["prompt"] {
  const ctx = useContext(DialogContext);
  if (!ctx) {
    throw new Error("usePrompt must be used within DialogProvider");
  }
  return ctx.prompt;
}

/** 同时取 confirm + prompt；用于需要两种交互的组件。 */
export function useAppDialog(): DialogApi {
  const ctx = useContext(DialogContext);
  if (!ctx) {
    throw new Error("useAppDialog must be used within DialogProvider");
  }
  return ctx;
}
