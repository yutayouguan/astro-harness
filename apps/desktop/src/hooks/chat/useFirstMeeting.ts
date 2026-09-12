import { useCallback, useContext, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { InterfaceTourReady } from "../../components/onboarding/InterfaceTourContext";
import { withDeadline, type OnboardingStateDto } from "../../lib/ui/onboarding";
import {
  canAutoStartFirstMeeting,
  FIRST_MEETING_SKILL,
  firstMeetingPrompt,
  type FirstMeetingState,
} from "../../lib/ui/firstMeeting";
import type { SendOpts } from "./useSend";

type Props = {
  locale: string;
  providerReady: boolean;
  empty: boolean;
  input: string;
  busy: boolean;
  sessionId: string | null;
  send: (opts?: SendOpts) => Promise<boolean | void>;
  setInput: (text: string) => void;
  openSession: (id: string) => Promise<unknown>;
};

/** One native persisted invitation. The normal chat owns streaming, HITL and history. */
export function useFirstMeeting(props: Props) {
  const ready = useContext(InterfaceTourReady);
  const [meeting, setMeeting] = useState<FirstMeetingState | null>(null);
  const [loaded, setLoaded] = useState(!isTauri());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [hidden, setHidden] = useState(false);
  const autoHandled = useRef(false);
  const lock = useRef(false);
  const current = useRef(props);
  current.current = props;

  useEffect(() => {
    if (!ready || !isTauri()) return;
    let disposed = false;
    void withDeadline(invoke<OnboardingStateDto>("get_onboarding_state"))
      .then((state) => {
        if (!disposed) setMeeting(state.first_meeting ?? null);
      })
      .catch(() => {
        if (!disposed) setError(true);
      })
      .finally(() => {
        if (!disposed) setLoaded(true);
      });
    return () => {
      disposed = true;
    };
  }, [ready]);

  const start = useCallback(async () => {
    if (lock.current) return;
    const p = current.current;
    if (!p.empty || p.busy || p.input.trim() || !p.providerReady) return;
    lock.current = true;
    setBusy(true);
    setError(false);
    try {
      const started = await p.send({
        text: firstMeetingPrompt(p.locale),
        attachments: [],
        requiredSkill: FIRST_MEETING_SKILL,
        beforeStart: async (sessionId) => {
          const latest = current.current;
          if (
            !latest.empty ||
            latest.busy ||
            latest.input.trim() ||
            !latest.providerReady
          )
            return false;
          const claimed = await withDeadline(
            invoke<boolean>("resolve_first_meeting", { sessionId }),
          );
          if (claimed) setMeeting({ status: "started", session_id: sessionId });
          else {
            const state = await withDeadline(
              invoke<OnboardingStateDto>("get_onboarding_state"),
            );
            setMeeting(state.first_meeting ?? null);
          }
          return claimed;
        },
      });
      if (!started) setError(true);
    } catch {
      setError(true);
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    if (
      autoHandled.current ||
      !canAutoStartFirstMeeting({ ...props, meeting, ready })
    )
      return;
    autoHandled.current = true;
    void start();
  }, [
    meeting,
    ready,
    props.providerReady,
    props.empty,
    props.input,
    props.busy,
    start,
  ]);

  const defer = async () => {
    if (lock.current) return;
    autoHandled.current = true;
    lock.current = true;
    setBusy(true);
    try {
      await withDeadline(invoke("resolve_first_meeting", { sessionId: null }));
      setMeeting((previous) =>
        previous?.status === "started"
          ? previous
          : { status: "deferred", session_id: null },
      );
      setHidden(true);
      setError(false);
    } catch {
      setError(true);
    } finally {
      lock.current = false;
      setBusy(false);
    }
  };

  const resume = async () => {
    if (lock.current) return;
    if (meeting?.session_id) {
      lock.current = true;
      setBusy(true);
      try {
        const history = await withDeadline(
          invoke<{ items: unknown[] }>("get_chat_history", {
            sessionId: meeting.session_id,
            limit: 1,
          }),
        );
        if (history.items.length === 0) {
          // Claim may have survived a crash before submission. Never auto-send a retry.
          const latest = current.current;
          if (!latest.empty || latest.input.trim()) return;
          latest.setInput(firstMeetingPrompt(latest.locale));
        } else await current.current.openSession(meeting.session_id);
        setHidden(true);
      } catch {
        setError(true);
      } finally {
        lock.current = false;
        setBusy(false);
      }
    } else {
      // A draft is never overwritten. Choosing to meet later doesn't send anything.
      if (!props.empty || props.input.trim()) return;
      await start();
    }
  };

  return {
    loaded,
    meeting,
    busy,
    error,
    visible: loaded && !hidden && !!meeting && props.empty,
    blockTour:
      !loaded ||
      meeting?.status === "pending" ||
      (meeting?.status === "started" && meeting.session_id === props.sessionId),
    start: resume,
    defer,
  };
}
