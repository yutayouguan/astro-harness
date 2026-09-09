# Desktop pet interaction contract

The floating pet is a companion, not a second editor window. It must not become
the key/main window when shown, updated, clicked or restored.

## Focus and click routing

- Native creation uses `focused(false)`, `focusable(false)` and first-mouse
  acceptance. Main also accepts its first click. On macOS, `show()` reaches
  `makeKeyAndOrderFront`; initial focus settings alone do not protect later shows.
- State refresh compares visibility, physical size and always-on-top before
  changing them. Animation frames never call window `show()` or `set_focus()`.
- Alpha hit testing uses the rendered canvas (or a bounded static-image buffer).
  Transparent pixels pass mouse events through; visible pixels and toolbar buttons
  remain interactive. A serialized 40ms pointer probe can re-enable mouse events
  after entering an ignored window; pointer-enter events alone cannot do this.
- Native pointer/hit-test commands reject callers other than `desktop-pet`.
  Cleanup cancels probes and restores input; failed probes fail open. Native input
  writes are serialized across StrictMode remounts.
- Asset images request anonymous CORS before loading. Tauri's asset protocol
  supplies the window origin header, permitting alpha reads without canvas taint.
- Dragging and pointer presses retain interaction. Keyboard users operate the pet
  from main-window Settings; the non-activating overlay does not claim a keyboard
  context-menu shortcut.

## Playback

- Main and optional grooming atlases decode ahead of action switches. A pending
  clip holds the previous visible pose instead of clearing the canvas.
- Action changes capture the currently displayed pose and use a brief 90ms blend.
  Premultiplied additive composition avoids an opacity dip in overlapping fur.
  Reduced motion skips this blend.
- Gaze follows the shortest angular arc with bounded catch-up and hysteresis at
  sprite boundaries. Pointer jitter does not restart the animation clock.
- Idle uses varied resting intervals before a short blink. Greeting ends after
  one complete 700ms loop. Static holds do not repaint identical canvas frames.
- This improves playback of the existing art; it does **not** add generated
  in-between frames or claim that crossfading is anatomical motion interpolation.

## Native acceptance checklist

After rebuilding and restarting, verify with the pet enabled:

1. Single clicks on main navigation tabs and buttons work, including after pet
   enable, resize, pause, restore and a pet gesture.
2. A control under transparent pet-window padding remains clickable; moving back
   over the pet re-enables dragging and the menu without a stuck ignored window.
3. Cross-monitor/DPI moves preserve hit testing. Main text-field focus remains
   stable during pet-state updates.
4. Blink cadence, gaze boundaries, kneading/grooming entry and exit do not flash;
   inspect whether existing source poses still need generated in-betweens.

Source/unit checks and a successful build do not replace this native acceptance.
