import { useContext, useEffect, useRef, useState } from "react";
import { isTauri, invoke } from "@tauri-apps/api/core";
import type { Driver, DriveStep } from "driver.js";
import { useI18n } from "../../i18n/LocaleContext";
import {
  INTERFACE_TOUR_REQUEST,
  interfaceTourCopy,
  shouldOfferInterfaceTour,
  type InterfaceTourState,
  type TourOutcome,
} from "../../lib/ui/interfaceTour";
import { withDeadline } from "../../lib/ui/onboarding";
import { InterfaceTourReady } from "./InterfaceTourContext";
import "driver.js/dist/driver.css";
import "../../styles/features/interface-tour.css";

function visibleTarget(id: string): HTMLElement | undefined {
  return Array.from(
    document.querySelectorAll<HTMLElement>(`[data-tour="${id}"]`),
  ).find((node) => {
    const box = node.getBoundingClientRect();
    return (
      box.width > 0 &&
      box.height > 0 &&
      box.right > 0 &&
      box.bottom > 0 &&
      box.left < window.innerWidth &&
      box.top < window.innerHeight &&
      getComputedStyle(node).visibility !== "hidden"
    );
  });
}

export default function InterfaceTour({
  available,
  onPrepare,
  onActiveChange,
}: {
  available: boolean;
  onPrepare: () => void;
  onActiveChange: (active: boolean) => void;
}) {
  const ready = useContext(InterfaceTourReady);
  const { locale } = useI18n();
  const copy = interfaceTourCopy[locale === "zh" ? "zh" : "en"];
  const [requested, setRequested] = useState(false);
  const [failedOutcome, setFailedOutcome] = useState<TourOutcome | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const [saving, setSaving] = useState(false);
  const callbacks = useRef({ onPrepare, onActiveChange });
  callbacks.current = { onPrepare, onActiveChange };
  const autoHandled = useRef(false);
  const alive = useRef(true);
  const saveGeneration = useRef(0);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  async function save(outcome: TourOutcome) {
    if (!isTauri()) return;
    const generation = ++saveGeneration.current;
    setSaving(true);
    try {
      await withDeadline(invoke("resolve_interface_tour", { outcome }));
      if (alive.current && generation === saveGeneration.current)
        setFailedOutcome(null);
    } catch {
      if (alive.current && generation === saveGeneration.current)
        setFailedOutcome(outcome);
    } finally {
      if (alive.current && generation === saveGeneration.current)
        setSaving(false);
    }
  }
  const saveRef = useRef(save);
  saveRef.current = save;

  useEffect(() => {
    const replay = () => {
      autoHandled.current = true;
      setUnavailable(false);
      callbacks.current.onPrepare();
      setRequested(true);
    };
    window.addEventListener(INTERFACE_TOUR_REQUEST, replay);
    return () => window.removeEventListener(INTERFACE_TOUR_REQUEST, replay);
  }, []);

  useEffect(() => {
    if (!ready || !isTauri() || autoHandled.current) return;
    let cancelled = false;
    void withDeadline(invoke<InterfaceTourState>("get_interface_tour_state"))
      .then((state) => {
        if (cancelled || autoHandled.current) return;
        autoHandled.current = true;
        if (state && shouldOfferInterfaceTour(state)) setRequested(true);
      })
      .catch(() => {
        // A broken progress read must not block the app or reset saved choices.
        // The explicit replay entry remains available.
      });
    return () => {
      cancelled = true;
    };
  }, [ready]);

  useEffect(() => {
    if (!ready || !available || !requested) return;
    let disposed = false;
    let tour: Driver | undefined;
    let frame = 0;
    let timer = 0;
    let root: HTMLElement | null = null;
    let wasInert = false;
    let released = false;
    const previousFocus = document.activeElement as HTMLElement | null;
    callbacks.current.onActiveChange(true);

    const finish = (outcome: TourOutcome) => {
      if (disposed || !tour?.isActive()) return;
      tour?.destroy();
      // Driver can remove its popover before its active-step bookkeeping is ready.
      // An immediate Escape then skips onDestroyed; always release our own state.
      release();
      setRequested(false);
      void saveRef.current(outcome);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (!tour?.isActive()) return;
      // Do not let app-wide shortcuts create tasks or send messages under a tour.
      event.stopImmediatePropagation();
      if (event.key === "Escape") {
        event.preventDefault();
        finish("skipped");
      } else if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
        event.preventDefault();
        if (event.key === "ArrowLeft") {
          if (tour.hasPreviousStep()) tour.movePrevious();
        } else if (tour.hasNextStep()) tour.moveNext();
        else finish("completed");
      } else if (event.key === "Tab") {
        event.preventDefault();
        const buttons = Array.from(
          document.querySelectorAll<HTMLButtonElement>(
            ".astro-interface-tour button:not([disabled])",
          ),
        ).filter((node) => node.getBoundingClientRect().width > 0);
        const current = buttons.indexOf(
          document.activeElement as HTMLButtonElement,
        );
        const next =
          (current + (event.shiftKey ? -1 : 1) + buttons.length) %
          buttons.length;
        buttons[next]?.focus();
      } else if (event.metaKey || event.ctrlKey) {
        event.preventDefault();
      }
    };

    const release = () => {
      if (released) return;
      released = true;
      cancelAnimationFrame(frame);
      clearTimeout(timer);
      window.removeEventListener("keydown", onKeyDown, true);
      if (root) root.inert = wasInert;
      callbacks.current.onActiveChange(false);
      if (previousFocus?.isConnected)
        previousFocus.focus({ preventScroll: true });
    };

    void import("driver.js")
      .then(({ driver }) => {
        if (disposed) return;
        // Wait for React's temporary sidebar expansion and any existing dialog to close.
        const deadline = performance.now() + 5000;
        const start = () => {
          if (disposed) return;
          if (document.querySelector('[role="dialog"][aria-modal="true"]')) {
            if (performance.now() < deadline) {
              timer = window.setTimeout(start, 100);
              return;
            }
            setUnavailable(true);
            setRequested(false);
            return;
          }
          const steps: DriveStep[] = copy.steps.flatMap(
            ([id, title, description]) => {
              const element = visibleTarget(id);
              return element
                ? [
                    {
                      element,
                      popover: {
                        title,
                        description,
                        side:
                          id === "composer" || id === "appearance"
                            ? "top"
                            : id === "toolbar"
                              ? "bottom"
                              : "right",
                      },
                    },
                  ]
                : [];
            },
          );
          if (!steps.length) {
            setUnavailable(true);
            setRequested(false);
            return;
          }
          root = document.querySelector(".app-shell");
          if (root) {
            wasInert = root.inert;
            root.inert = true;
          }
          tour = driver({
            // Instant positioning also avoids motion during keyboard navigation.
            animate: false,
            smoothScroll: false,
            disableActiveInteraction: true,
            allowKeyboardControl: false,
            overlayClickBehavior: () => {},
            overlayOpacity: 0.45,
            stagePadding: 6,
            stageRadius: 12,
            popoverClass: "astro-interface-tour",
            showProgress: false,
            progressText: "{{current}} / {{total}}",
            nextBtnText: copy.next,
            prevBtnText: copy.previous,
            doneBtnText: copy.done,
            onCloseClick: () => finish("skipped"),
            onNextClick: () => {
              if (tour?.hasNextStep()) tour.moveNext();
              else finish("completed");
            },
            onDestroyed: () => {
              release();
              if (!disposed) setRequested(false);
            },
            onPopoverRender: (popover, { state }) => {
              const welcome = state.activeIndex === 0;
              popover.closeButton.textContent = welcome
                ? copy.dismiss
                : copy.skip;
              popover.closeButton.setAttribute(
                "aria-label",
                welcome ? copy.dismiss : copy.skip,
              );
              popover.wrapper.setAttribute("aria-modal", "true");
              popover.title.id = "astro-tour-title";
              popover.description.id = "astro-tour-description";
              popover.wrapper.setAttribute("aria-labelledby", popover.title.id);
              popover.wrapper.setAttribute(
                "aria-describedby",
                popover.description.id,
              );
              // Driver positions the popover after this hook; focus on the next frame.
              frame = requestAnimationFrame(() => {
                if (!disposed) popover.nextButton.focus();
              });
            },
            steps: [
              {
                popover: {
                  title: copy.welcome,
                  description: copy.intro,
                  nextBtnText: copy.begin,
                  showProgress: false,
                  showButtons: ["next", "close"],
                },
              },
              ...steps.map((step, index) => ({
                ...step,
                popover: {
                  ...step.popover,
                  showProgress: true,
                  progressText: `${index + 1} / ${steps.length}`,
                },
              })),
            ],
          });
          window.addEventListener("keydown", onKeyDown, true);
          tour.drive();
        };
        frame = requestAnimationFrame(() => {
          frame = requestAnimationFrame(start);
        });
      })
      .catch(() => {
        if (!disposed) {
          setUnavailable(true);
          setRequested(false);
        }
      });

    return () => {
      disposed = true;
      tour?.destroy(); // Cleanup is not a user decision: never persist a skip here.
      release();
    };
  }, [ready, available, requested, copy]);

  if (!failedOutcome && !unavailable) return null;
  return (
    <div className="interface-tour-notice" role="alert">
      <span>{failedOutcome ? copy.saveError : copy.unavailable}</span>
      {failedOutcome && (
        <button
          type="button"
          disabled={saving}
          onClick={() => void save(failedOutcome)}
        >
          {copy.retry}
        </button>
      )}
      <button
        type="button"
        onClick={() => {
          setFailedOutcome(null);
          setUnavailable(false);
        }}
      >
        {copy.close}
      </button>
    </div>
  );
}
