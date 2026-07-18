# Astro Interface System

## Direction

Calm, dense chat tool. Status is glanceable; chrome stays quiet. Prefer Lucide line icons and existing CSS tokens (`--ink`, `--ink-soft`, `--tone-blue`) over new accent colors.

## Depth

Borders-only / soft surface tints. No dramatic shadows on list rows.

## Spacing

Base unit ~4px. Session row status slot is a fixed 14×14 grid.

## Component patterns

### Chat session list — status leading icon

Row prefix communicates generation state. Do not leave the leading slot empty for idle sessions.

| State | Icon | Class / treatment |
| --- | --- | --- |
| Running (`streamingSessionId === sessionId`) | `LoaderCircle` 14px, stroke 2.2 | `.chat-session-status-spin` — `--tone-blue`, continuous spin |
| Complete (not streaming) | `CircleCheck` 14px, stroke 2 | `.chat-session-status-complete` — `--ink-soft` |
| Complete + unread | same `CircleCheck` | row `.is-unread`: icon `--tone-blue`; title `font-weight: 700` |

Rules:

- Unread is an emphasis on the completed icon + title weight, not a separate blue dot.
- Status slot stays 14×14 so titles stay aligned across states.
- Icons are decorative (`aria-hidden`); the row button carries the action.
- Quick actions (pin / archive / more) stay on the right; do not move status there.

Source: `ChatSessionList.tsx`, `right-panel.css` (`.chat-session-status*`).

### Chat welcome — brand mark

Empty-session hero uses the product logo, not an empty-state illustration.

- Component: `AstroLogoMark` at 72×72 inside `.chat-welcome-mark` (88×88 slot)
- Soft radial glow behind the mark; gentle float animation
- Wordmark “Astro Agent” sits under the mark; greeting title remains the primary text
- Do not put cards or secondary chrome in the brand stack

Source: `ChatWelcome.tsx`, `markdown.css` (`.chat-welcome-mark*`, `.chat-welcome-logo`).
