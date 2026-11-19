# Velour design system

Inspired by the author's Tiphys app: near-black canvas, translucent cards with hairline borders,
large radii, indigo accent, Plus Jakarta Sans. Dark first; light mirrors Tiphys light.
No backdrop blur: GTK can't do it cheaply, so depth comes from layered translucency and two
very faint static color glows drawn once on the window background.

## Color tokens (CSS custom properties in `crates/app/src/style.css`)

| Token | Dark | Light | Use |
|---|---|---|---|
| `bg` | `#000000` + glows (indigo 7 %, orange 3.5 %) | `#F2F2F7` | window canvas |
| `card` | white 6 % | `#FFFFFF` | sidebar, bar, side panel, track lists, settings groups |
| `card-border` | white 12 % | none | 1 px card outline |
| `hover` / `press` | white 7 % / 12 % | black 5 % / 9 % | interactive states |
| `text` | `#F5F5F7` | `#111114` | primary text |
| `text2` | `#98989F` | `#6C6C70` | secondary text |
| `accent` | `#A5B4FC` | `#4338CA` | progress, playing row, liked, switches, focus |
| `accent-tile` | `#5E5CE6` 30 % + indigo border | `#1E1B4B` | playing row background |
| `solid` / `on-solid` | `#F5F5F7` / black | `#111114` / white | play button, primary pill, selected nav row, active segment |
| `danger` | `#FF6961` | `#C62828` | destructive actions |

Contrast: text2 on black ≈ 7:1; accent on black ≈ 10:1; black on `solid` ≈ 19:1.

## Typography

Plus Jakarta Sans (OFL, bundled in `assets/fonts`, installed to `~/.local/share/fonts/velour`),
fallback Inter. Weight carries hierarchy; tabular numbers for times.

| Role | Size / weight |
|---|---|
| Large title | 34 / 800, −0.6 tracking |
| Brand | 22 / 800 |
| Section title | 20 / 700 |
| Body / rows | 14 / 600 title, 12 / 500 secondary |
| Section label | 12 / 600, uppercase, +0.6 tracking |

## Shape & spacing

- Radii: cards 22–26, tiles 20, covers 16 (header 22), thumbnails 10, buttons 12–14, pills 999.
- Shell padding 12 px; 12 px gaps between floating cards. Page padding 60/36/40 (top leaves room
  for the floating back button).
- Motion: 120–200 ms ease-out on hover/press and panel reveal; nothing decorative.

## Layout

Floating cards on the canvas: sidebar card (brand, search, nav, PLAYLISTS) · content (no card,
sits on the canvas) · optional side panel card (queue or lyrics) · now-playing card at the bottom.
Sidebar collapses to icons under 900 px window width.

## Components

Nav row (selected = solid white pill) · track list card with rows (playing row = indigo tile) ·
cover tile · mood chip · header with cover, title, actions (primary pill = solid) · segmented
pill tabs · now-playing card · side panel · popover menus (#1C1C1E, radius 18) · toast.

## States

Loading skeletons with fixed sizes · empty state · error with "Try again" · unavailable rows at
40 % opacity, not activatable · long text ellipsized with full text in tooltips.

## Accessibility

Symbolic icons (Material Symbols, bundled), every icon-only button has tooltip + accessible label,
visible 2 px focus ring in the accent color, full keyboard control (see Settings → About).
