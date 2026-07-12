# Adaptive Chat Layout Design

## Goal

Keep the chat canvas and Navbar absolutely centered with the same responsive width, while preventing the expandable history rail from changing the chat canvas position or width on desktop.

## Context

`SiteNavbar` currently uses `max-w-5xl`, while `ChatWorkspace` uses `max-w-7xl`. The history card is a normal flex sibling of `data-slot="chat-main"`, so opening history changes the available width and visual center of the chat.

The UI must adapt to CSS viewport width across 13-inch Mac displays and larger 27/32-inch monitors. Physical screen size is not a reliable CSS input; viewport width is the responsive source of truth.

## Chosen design

- Define shared global layout tokens in `wyse-web/app/app.css`.
- Use one responsive `--content-width` token for both Navbar and chat canvas:
  - page gutters use `clamp()`;
  - content width grows with viewport width up to `64rem` / `1024px`;
  - below the available viewport width it naturally shrinks without horizontal overflow.
- Use shared spacing and body-size tokens for the layout values that need to scale with viewport width.
- Keep `data-slot="chat-main"`, its message scroller, composer placement, and full-height behavior intact.
- On desktop, position the history rail outside the centered chat canvas so it does not participate in the chat width calculation.
- On mobile, keep the history rail in normal flow above the chat canvas, where expanding it may increase page content height.

## Alternatives considered

1. Keep a `max-w-7xl` flex row and reduce the history width. Rejected: history still changes the chat canvas width and center.
2. Make the history rail fixed to the viewport. Rejected: it loses alignment with the chat section and creates additional scroll/overlap behavior.
3. Use a shared centered canvas plus an outside desktop history rail. Chosen: it preserves the primary chat view and needs no new state or dependency.

## Acceptance criteria

- Navbar and chat canvas have the same left and right edges at desktop widths.
- Expanding or collapsing history does not move or resize the desktop chat canvas.
- At mobile widths there is no horizontal scrolling; history remains usable above chat.
- Typography, gutters, and gaps use global responsive tokens instead of duplicated viewport-specific magic values.
- Existing chat layout guardrails and tests remain valid.
