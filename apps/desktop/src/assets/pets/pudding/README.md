# 布丁 / Pudding

Offline cream golden-retriever puppy with caramel floppy ears, no accessories.
Stable library id: `builtin-pudding`. Generated with authorized Azure gpt-image-2,
using imagegen CLI and JPEG100 transport; final files are local RGBA WebP assets.
No user photo or credential is bundled. Naitang remains the onboarding default.

Main atlas: v2, 1536×2288, 9 standard state rows and 16 gentle look directions.
Failed prop-bearing, clipped and wrong-direction sources are excluded. Look
poses are selected from multiple real generated families and registered to the
same neutral; source-per-direction provenance is retained in the run QA files.
This follows the user's explicit decision to use hatch-pet as a reference, not
require a single-generation row or fixed independent-clip frame count.

| Clip | Generated source poses | Used generated poses | Playback positions |
| --- | ---: | ---: | ---: |
| tail-wag v2 | no new model request | original neutral tail texture | 49 baked frames |
| head-tilt | 20 | 12 | 24 |
| stretch | 24 | 22 | 24 |
| nap | 24 | 20 | 22 |

Playback positions include explicit reverse returns and the shared neutral at
both ends; they are not claims of extra AI-generated in-betweens. Each clip
contains its own entry/loop/exit durations and `neutralBookends`. Nap uses body
centering for the lying pose. Every output has full-body transparency, with no
CSS shadow added by the player.

Tail-wag v2 replaces the drifting whole-puppy frames with a layered bake from the
approved neutral. Head, body, paws and their antialiased edges are byte-identical
in every frame. Only the separated original tail texture moves around a fixed
root, with eased entry/exit and a 32-frame sinusoidal loop. Tests verify protected
pixels are unchanged, the tail remains connected and the canvas has no clipped
edges. The other three motion assets and the main atlas are unchanged.

Managed runtime filename is `tail-wag-v2.webp` to invalidate decoded-image caches.
Startup upgrades only the exact old shipped tail bytes, metadata and unchanged
main art. Old files are retained; user-edited assets/timings, names, placement,
size and scene preferences are not overwritten. Source/QA and a rollback copy:
`output/imagegen/pudding-tail-fixed-20260911/`.

Run: `output/imagegen/pudding-dog-20260910/`. Prompts, source images, rejected
attempts, `motion-recipes.json`, per-clip `.qa.json`, white/black/checker previews,
normal/slow GIFs, selected gaze provenance and validation reports are retained.
Independent visual QA passed with minor pose/edge observations. Native realtime
focus and visual acceptance is tracked in `docs/desktop-pet-pudding-todo.md`.
