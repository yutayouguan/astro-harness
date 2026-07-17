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
  confirmLabel?: string;
  cancelLabel?: string;
  variant?: "default" | "danger";
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
  resolve: (ok: boolean) => void;
};

type PromptRequest = {
  kind: "prompt";
  options: PromptOptions;
  resolve: (value: string | null) => void;
};

type DialogRequest = ConfirmRequest | PromptRequest;

type DialogApi = {
  confirm: (options: ConfirmOptions) => Promise<boolean>;
  prompt: (options: PromptOptions) => Promise<string | null>;
};

const DialogContext = createContext<DialogApi | null>(null);

export function DialogProvider({ children }: { children: ReactNode }) {
  const { t } = useI18n();
  const [request, setRequest] = useState<DialogRequest | null>(null);
  const [draft, setDraft] = useState("");
  const queueRef = useRef<DialogRequest[]>([]);
  const activeRef = useRef<DialogRequest | null>(null);

  const pump = useCallback(() => {
    if (activeRef.current) return;
    const next = queueRef.current.shift() ?? null;
    activeRef.current = next;
    if (!next) {
      setRequest(null);
      setDraft("");
      return;
    }
    if (next.kind === "prompt") {
      setDraft(next.options.defaultValue ?? "");
    } else {
      setDraft("");
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
      // 下一帧再弹出队列，避免同一次点击连环触发
      window.setTimeout(() => pump(), 0);
    },
    [pump],
  );

  const confirm = useCallback(
    (options: ConfirmOptions) =>
      new Promise<boolean>((resolve) => {
        enqueue({ kind: "confirm", options, resolve });
      }),
    [enqueue],
  );

  const prompt = useCallback(
    (options: PromptOptions) =>
      new Promise<string | null>((resolve) => {
        enqueue({ kind: "prompt", options, resolve });
      }),
    [enqueue],
  );

  const api = useMemo(() => ({ confirm, prompt }), [confirm, prompt]);

  const cancelLabel =
    request?.options.cancelLabel ?? t("dialog.cancel");
  const confirmLabel =
    request?.kind === "prompt"
      ? (request.options.confirmLabel ?? t("dialog.save"))
      : (request?.options.confirmLabel ?? t("dialog.confirm"));

  const promptEmpty =
    request?.kind === "prompt" &&
    !request.options.allowEmpty &&
    !draft.trim();

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
              closeActive(() => request.resolve(false));
            } else {
              closeActive(() => request.resolve(null));
            }
          }}
          onConfirm={() => {
            if (request.kind === "confirm") {
              closeActive(() => request.resolve(true));
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
