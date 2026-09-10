# Desktop pet interaction contract

The floating pet is a companion, not a second editor window. It must not become
the key/main window when shown, updated, clicked or restored.

## Focus and click routing

The size slider displays 75%–150%, with a 100% default equal to the former 40%
size. Internal scale remains physical: 0.30–0.60, default0.40. Thus the default
window is120×136 logical pixels, maximum180×204, minimum90×102. Existing0.40
choices stay the same size; old default1.0 reads as0.40, other formerly valid
oversized values cap at0.60. Reads do not rewrite saved files; the next settings
transaction persists the normalized value. Native, tools and UI share bounds.

- Native creation uses `focused(false)`, `focusable(false)` and first-mouse
  acceptance. Main also accepts its first click. On macOS, `show()` reaches
  `makeKeyAndOrderFront`; initial focus settings alone do not protect later shows.
- State refresh compares visibility, physical size and always-on-top before
  changing them. Animation frames never call window `show()` or `set_focus()`.
- Alpha hit testing uses the rendered canvas (or a bounded static-image buffer).
  Transparent pixels pass mouse events through; only visible pet pixels
  remain interactive. No overlay toolbar is drawn over the character; open,
  settings and hide actions live in the right-click menu. A serialized 40ms pointer probe can re-enable mouse events
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
- Crossfading is not anatomical interpolation. Independent clips additionally
  have newly generated pose families; the short blend only smooths sampling.

## Independent Astro motion clips

`pet.json`, shared state and saved scene identity optionally carry `motionClips`.
Each entry has `path`, `frameWidth`, `frameHeight`, `columns`, `durationsMs`,
`loopStart`, `loopEnd` (exclusive) and `loopRepeats`. Entry and exit play once;
only the loop range repeats. Each clip owns its grid and frame count. Import
validates package-relative paths, bounded metadata, image dimensions, alpha,
nonempty used cells and empty unused cells. Export rewrites paths relative to
the package; applying a scene validates managed paths and carries its motions.

The built-in has independently generated kneading/grooming pose families,
assembled into 16/17 playback frames. Selected poses are reused for reverse
entry/exit; generation QA records every source index and unique-pose count.
The managed built-in directory is versioned. Only an untouched previous built-in
is upgraded; visibility, scale, pause, wallpaper linkage and scenes survive.
Imported/custom pets are never silently replaced.

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
