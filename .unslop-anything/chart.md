# Unslop chart

Light pass 2026-11-25. Direction: unslop only; keep the GODsend family look.

## Accepted exceptions (brand / design language)
- Laser gradient (slop-gradient, gradient-text, glow-shadow, glass-everywhere) on the logo, primary button and hero: the GODsend family identity.
- Tracked mono caps eyebrows and labels: part of the console-HUD voice shared by all GODsend apps.
- hover-only on docs links: hover styles now also apply on :focus-visible; links are natively focusable.
- clickable-div on Files rows: rows hold their own Open/Select buttons, so they stay divs with tabIndex, aria-current and an Enter/Space handler instead of role=button (nested interactives).
- macOS overlay title bar inset: untouched.
