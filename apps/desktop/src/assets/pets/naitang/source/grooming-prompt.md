Use case: precise-object-edit
Asset type: fixed-camera 24-frame transparent desktop-cat grooming animation, laid out exactly 4 columns x 6 rows in a 1024x1536 image.
Input image is the EDIT TARGET, not a loose style reference. It repeats the exact same orange-white kitten in every 256x256 cell. The editable mask covers only the kitten's screen-left foreleg/forepaw and a small mouth area. Everything else MUST stay unchanged: identical face and eyes, fur pattern, head tilt, body, other three paws, tail, scale, lighting, ground baseline, camera and precise cell placement.
Animate only that forepaw and the mouth. The kitten gently lifts the screen-left forepaw, folds the elbow and wrist to bring the paw to the mouth, makes several tiny tongue-to-paw licks, pauses, then lowers and plants that same paw back in its original exact position. The opposite forepaw supports the body throughout. When the forepaw lifts, repair the newly revealed fur/background behind it inside the mask. Do not leave a second paw at the old location. Four paws total, never extra toes or limbs. Do not move the head to meet the paw; bring the paw naturally to the mouth.
Read frames row-major, left to right then top to bottom:
0 original untouched neutral.
1–5 progressive wrist/elbow fold and paw lift from floor to mouth, six gentle intermediate phases.
6–17 sustained grooming: paw near mouth, three subtle tongue extension/contact/retraction cycles, small wrist rotations. Tongue must touch the paw, not lick empty space. Paw pose at 17 should connect back to 6 for a repeatable grooming loop.
18–22 gradual reverse lowering with tongue fully retracted and mouth closed.
23 exactly the original untouched neutral.
Return the same grid, with no labels, borders or text. Keep exactly flat pure magenta RGB(255,0,255) wherever there is background. No cast shadow, floor, glow, gradients, white rectangle or background scenery. Keep the original realistic soft fur rendering. Do not rescale, translate or redraw the entire kitten.
