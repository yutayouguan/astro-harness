# Desktop pet interaction contract

The floating pet is a companion, not a second editor window. It must not become
the key/main window when shown, updated, clicked or restored.

## Focus and click routing

The size slider displays 50%–100%, with a 100% default and ceiling equal to the
previous 75% size (physical scale 0.30). Internal scale remains physical:
0.15–0.30. The default/maximum window is 90×102 logical pixels; minimum is 45×51.
Supported saved values remain unchanged. Formerly valid oversized values in the
live state, pet defaults, and scene overrides are capped to 0.30 on read; old
exported packages receive the same cap on import. Malformed/out-of-range values
outside the formerly supported ranges still fail validation.
Size normalization alone does not rewrite saved files;
the pet-library migration below persists it with the backed-up state transition.
Native, tools and UI share bounds.

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

## Placement and quiet behavior

Shared state has validated `preferences`: normalized per-monitor `position`,
`positionLocked`, `snapToEdge`, `quietMode`, `hideInFullscreen`, `presentationMode`
and `activityIntervalSecs` (15–300). Native drag completion waits for mouse release
on macOS/Windows; geometry uses monitor work areas and supports negative origins.
Missing monitors fall back to the primary screen. Placement updates are idempotent
and do not run on every animation frame. "Bring back on screen" resets the anchor.

Quiet mode stops automatic large actions, not explicit manual previews. Presentation
and fullscreen suppression do not change `enabled`; tray recovery clears presentation
and temporarily overrides the current fullscreen interval. macOS observes only
foreground window bounds/PID, not titles, screenshots or accessibility content.
Other platforms support Astro fullscreen/manual presentation; physical platform
acceptance is recorded separately in `desktop-pet-todo.md`.

Scenes optionally capture `preferences` including scale and behavior. Only pet/all/
linked-pet application restores these. Presentation and fullscreen hiding remain global. Exported
`pet.json` carries `scenePreferences`; import validates and saves to the library without applying.
Scenes without preferences inherit their pet's defaults. Wallpaper generation's concurrency
check ignores preference-only changes but keeps the newest preference snapshot.

## Pet management center

Settings opens on Library, with Create and General as separate keyboard-accessible
tabs. Selecting a card opens that pet's Scenes / Motion & defaults details without
changing the desktop. Only explicit apply actions change the active pet or scene.
The current-desktop strip remains independent from the selected management card.

`pets: PetRecord[]` owns a stable id, identity and defaults; `scene.pet.petId` is the
association. Shared transactions synchronize scene identity snapshots from that
record, while scene preferences are either an override or null (inherit). Global
visibility, topmost, fullscreen hiding and presentation intent are not overridden by
scene preferences. `activePetId` and `activeSceneId` track the applied selection.

Legacy state with no `libraryVersion` is upgraded under `state.lock` with an exact
`state.before-pet-library.json` backup before replacing state. Scene ids, names,
configuration and asset files remain intact. New static generation and package
import register independent library entries. Built-in Naitang is seeded without
enabling or switching the active pet. Deleting a scene does not remove its pet;
deleting a custom pet confirms the current number of associated scenes, hides it
if active, removes those library records, and intentionally retains all asset files.

See `desktop-pet-library-todo.md` and `desktop-pet-pudding-todo.md` for verified
checks and outstanding native acceptance. Pudding is now a second offline built-in
with validated real image/animation assets; seeding checks each catalog identity
without changing the active pet. Both Settings and the native context menu derive
their action list from the current pet's supported clips. Pudding has tail-wag,
head-tilt, stretch and nap; it does not inherit cat kneading/grooming actions.
Scene export snapshots inherited effective preferences for portable packages,
without changing the saved scene's inheritance or applying it to the desktop.

## Playback details

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

`neutralBookends` is explicit per clip. Built-in clips use the exact main-atlas
neutral frame at both ends, eliminating the generated-lookalike seam. Only unchanged
built-in art and timings are upgraded; custom clips and their bookends are preserved.

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
