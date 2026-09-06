/** Portal-based time picker whose layer always stays above its owning drawer. */
import { useEffect, useId, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { Check, Clock } from "lucide-react";
import {
  ChevronDown as ChevronDownData,
  ChevronUp as ChevronUpData,
} from "lucide";
import { useAnchoredMenu } from "../../hooks/ui/useAnchoredMenu";
import { useDynamicOverlayLayer } from "../../hooks/ui/useDynamicOverlayLayer";
import { useI18n } from "../../i18n/LocaleContext";
import { toneStyleFromElement } from "../../lib/ui/toneFromElement";
import { MorphToggleIcon } from "../icons/MorphIcon";

type Props = {
  value: string;
  onChange: (next: string) => void;
  "aria-label"?: string;
};

const HOURS = Array.from({ length: 24 }, (_, index) => index);
const MINUTES = Array.from({ length: 60 }, (_, index) => index);

function pad2(value: number) {
  return String(value).padStart(2, "0");
}

function parseTime(value: string) {
  const match = /^(\d{1,2}):(\d{2})$/.exec(value.trim());
  const hour = Number(match?.[1] ?? 9);
  const minute = Number(match?.[2] ?? 0);
  return {
    hour: Math.min(23, Math.max(0, Number.isFinite(hour) ? hour : 9)),
    minute: Math.min(59, Math.max(0, Number.isFinite(minute) ? minute : 0)),
  };
}

export function GlassTimePicker({
  value,
  onChange,
  "aria-label": ariaLabel,
}: Props) {
  const { t, locale } = useI18n();
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const hourRef = useRef<HTMLButtonElement>(null);
  const minuteRef = useRef<HTMLButtonElement>(null);
  const popId = useId();
  const { hour, minute } = parseTime(value);
  const { layer, bringToFront } = useDynamicOverlayLayer(open);

  const pos = useAnchoredMenu({
    open,
    anchorRef: triggerRef,
    menuRef: popRef,
    fixedWidth: 224,
    preferAlign: "start",
    placement: "auto",
    gap: 8,
    maxHeightCap: 276,
    maxHeightRatio: 0.72,
    minMaxHeight: 180,
  });

  const close = (restoreFocus = false) => {
    setOpen(false);
    if (restoreFocus) triggerRef.current?.focus();
  };

  useEffect(() => {
    if (!open) return;
    const frame = requestAnimationFrame(() => {
      hourRef.current?.scrollIntoView({ block: "center" });
      minuteRef.current?.scrollIntoView({ block: "center" });
    });
    const onDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (
        triggerRef.current?.contains(target) ||
        popRef.current?.contains(target)
      )
        return;
      close();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close(true);
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const label = ariaLabel || t("cron.time");
  const toneStyle = open ? toneStyleFromElement(triggerRef.current) : {};
  const popup = open
    ? createPortal(
        <div
          ref={popRef}
          id={popId}
          className="cron-dt-pop cron-time-pop"
          role="dialog"
          aria-label={label}
          style={
            pos
              ? {
                  top: pos.top,
                  left: pos.left,
                  width: pos.width,
                  maxHeight: pos.maxHeight,
                  zIndex: layer,
                  ...toneStyle,
                }
              : {
                  visibility: "hidden",
                  width: 224,
                  zIndex: layer,
                  ...toneStyle,
                }
          }
          onPointerDownCapture={bringToFront}
        >
          <div className="cron-time-columns">
            <TimeColumn
              label={locale === "zh" ? "时" : "Hour"}
              values={HOURS}
              selected={hour}
              selectedRef={hourRef}
              onSelect={(nextHour) =>
                onChange(`${pad2(nextHour)}:${pad2(minute)}`)
              }
            />
            <TimeColumn
              label={locale === "zh" ? "分" : "Minute"}
              values={MINUTES}
              selected={minute}
              selectedRef={minuteRef}
              onSelect={(nextMinute) => {
                onChange(`${pad2(hour)}:${pad2(nextMinute)}`);
                close(true);
              }}
            />
          </div>
        </div>,
        document.body,
      )
    : null;

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className={`cron-sched-field-shell cron-sched-field-shell--time cron-time-trigger${open ? " is-open" : ""}`}
        aria-label={label}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={popId}
        onClick={() => setOpen((current) => !current)}
      >
        <Clock size={14} strokeWidth={2.2} aria-hidden />
        <span className="cron-time-trigger-value">{`${pad2(hour)}:${pad2(minute)}`}</span>
        <MorphToggleIcon
          active={open}
          activeIcon={ChevronUpData}
          inactiveIcon={ChevronDownData}
          size={14}
          strokeWidth={2.2}
          aria-hidden
        />
      </button>
      {popup}
    </>
  );
}

function TimeColumn({
  label,
  values,
  selected,
  selectedRef,
  onSelect,
}: {
  label: string;
  values: number[];
  selected: number;
  selectedRef: RefObject<HTMLButtonElement>;
  onSelect: (value: number) => void;
}) {
  return (
    <div className="cron-time-column-wrap">
      <span className="cron-time-column-label">{label}</span>
      <div className="cron-time-column" role="listbox" aria-label={label}>
        {values.map((candidate) => {
          const active = candidate === selected;
          return (
            <button
              key={candidate}
              ref={active ? selectedRef : undefined}
              type="button"
              className={`cron-time-option${active ? " is-selected" : ""}`}
              role="option"
              aria-selected={active}
              onClick={() => onSelect(candidate)}
            >
              <span>{pad2(candidate)}</span>
              {active ? (
                <Check size={12} strokeWidth={2.6} aria-hidden />
              ) : null}
            </button>
          );
        })}
      </div>
    </div>
  );
}
