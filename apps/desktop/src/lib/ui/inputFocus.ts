/** Focus ownership is shared by all input surfaces; visual rules remain material-scoped. */
export const INPUT_FOCUS_CONTROL = [
  'input:not([type="hidden"], [type="checkbox"], [type="radio"], [type="range"], [type="color"], [type="file"], [type="button"], [type="submit"], [type="reset"], [type="image"], :disabled)',
  "textarea:not(:disabled)",
  "select:not(:disabled)",
  "button.select-menu-trigger:not(:disabled)",
  "button.cron-sched-field-shell:not(:disabled)",
  // 文档编辑器的可编辑区（CodeMirror .cm-content）不套输入框焦点环：
  // 打开文件后光标本身就是焦点提示，整块描边会和内容混在一起。
  '[contenteditable="true"][role="textbox"]:not(.cm-content)',
].join(", ");

// Only actual input shells, never a whole form/card. New composites can opt in
// with data-input-surface instead of inventing another focus implementation.
export const INPUT_FOCUS_SURFACES = [
  "[data-input-surface]",
  ".expandable-search-field",
  ".sidebar-settings-search",
  ".select-menu-search",
  ".pet-library-search",
  ".model-market-search",
  ".project-files-search",
  ".composer-plus-search",
  ".prefs-diag-search",
  ".loop-palette-search-wrap",
  ".cron-sched-field-shell",
  ".cron-sched-stepper",
].join(", ");

export function resolveInputFocusSurface(control: HTMLElement): HTMLElement {
  if (control.matches(".composer-input")) {
    return (
      control.closest<HTMLElement>(".composer:not(.has-clarify)") ?? control
    );
  }
  return control.closest<HTMLElement>(INPUT_FOCUS_SURFACES) ?? control;
}

export function isFocusNavigation(
  event: Pick<
    KeyboardEvent,
    "key" | "altKey" | "ctrlKey" | "metaKey" | "isComposing"
  >,
) {
  // Typing, caret arrows, shortcuts and IME must not turn a mouse-focused field
  // into a thick keyboard-navigation ring. Shift+Tab is still navigation.
  return (
    event.key === "Tab" &&
    !event.altKey &&
    !event.ctrlKey &&
    !event.metaKey &&
    !event.isComposing
  );
}

export function installInputFocus(doc: Document): () => void {
  const root = doc.documentElement;
  let control: HTMLElement | null = null;
  let surface: HTMLElement | null = null;
  const previousModality = root.dataset.focusModality;
  let restoreBox: (() => void) | undefined;
  root.dataset.focusModality = "pointer";

  const clear = () => {
    restoreBox?.();
    restoreBox = undefined;
    control?.removeAttribute("data-input-focus-control");
    surface?.removeAttribute("data-input-focus");
    control = null;
    surface = null;
  };
  const focus = (target: EventTarget | null) => {
    clear();
    if (
      !(target instanceof HTMLElement) ||
      !target.matches(INPUT_FOCUS_CONTROL)
    )
      return;
    control = target;
    surface = resolveInputFocusSurface(control);
    // Keep the padding+border footprint stable when the gradient ring changes
    // from a 1px pointer edge to a 2px keyboard edge. Never wrap/reparent inputs.
    restoreBox = prepareInputFocusBox(surface, doc.defaultView);
    control.setAttribute("data-input-focus-control", "");
    surface.setAttribute("data-input-focus", "");
  };
  const onFocusIn = (event: FocusEvent) => focus(event.target);
  const onFocusOut = () => clear();
  const onPointerDown = () => {
    root.dataset.focusModality = "pointer";
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (isFocusNavigation(event)) root.dataset.focusModality = "keyboard";
  };

  focus(doc.activeElement);
  doc.addEventListener("pointerdown", onPointerDown, true);
  doc.addEventListener("keydown", onKeyDown, true);
  doc.addEventListener("focusin", onFocusIn, true);
  doc.addEventListener("focusout", onFocusOut, true);
  return () => {
    clear();
    doc.removeEventListener("pointerdown", onPointerDown, true);
    doc.removeEventListener("keydown", onKeyDown, true);
    doc.removeEventListener("focusin", onFocusIn, true);
    doc.removeEventListener("focusout", onFocusOut, true);
    if (previousModality === undefined) delete root.dataset.focusModality;
    else root.dataset.focusModality = previousModality;
  };
}

const FOCUS_SIDES = ["top", "right", "bottom", "left"] as const;

export function prepareInputFocusBox(
  surface: HTMLElement,
  view: Window | null,
) {
  if (!view) return;
  const computed = view.getComputedStyle(surface);
  const spacing = FOCUS_SIDES.map(
    (side) =>
      parseFloat(computed.getPropertyValue(`padding-${side}`)) +
      parseFloat(computed.getPropertyValue(`border-${side}-width`)),
  );
  // A bare contenteditable has no padding to borrow; keep its native outline.
  if (spacing.some((value) => !Number.isFinite(value) || value <= 0)) return;
  const properties = [
    ...FOCUS_SIDES.map((side) => `--input-focus-box-${side}`),
    "--input-focus-box-budget",
  ];
  const previous = properties.map((property) => {
    return [
      property,
      surface.style.getPropertyValue(property),
      surface.style.getPropertyPriority(property),
    ] as const;
  });
  FOCUS_SIDES.forEach((side, index) => {
    surface.style.setProperty(
      `--input-focus-box-${side}`,
      `${spacing[index]}px`,
    );
  });
  surface.style.setProperty(
    "--input-focus-box-budget",
    `${Math.min(...spacing)}px`,
  );
  surface.setAttribute("data-input-focus-box", "");
  return () => {
    surface.removeAttribute("data-input-focus-box");
    for (const [property, value, priority] of previous) {
      if (value) surface.style.setProperty(property, value, priority);
      else surface.style.removeProperty(property);
    }
  };
}
