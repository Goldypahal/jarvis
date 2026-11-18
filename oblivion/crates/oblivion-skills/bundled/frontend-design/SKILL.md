---
name: frontend-design
description: Design and build web pages and app interfaces that look deliberate and polished rather than generic - landing pages, dashboards, forms, components - in HTML/CSS, React, Vue or Svelte. Use when the user asks for a website, page, UI, component or redesign, or says the current UI looks plain or "AI-generated".
license: MIT
---
# Front-end design

## 1. Decide the direction before writing code
Write three lines: **who it is for**, **the one thing they should do or feel**, and **an aesthetic**
that fits (for example: calm editorial, bold product launch, dense pro tool, playful, technical/terminal,
luxurious minimal). Commit to it; a clear direction beats safe defaults.

## 2. Build a small system, then use it everywhere
- **Type**: two families at most. Pick characterful fonts that suit the direction.
  **Local assets only**: never link to external font CDNs (`fonts.googleapis.com`); bundle fonts locally under `/static/fonts/` or use system font stacks (`-apple-system`, `Segoe UI`, `Roboto`) to preserve privacy and offline operation.
  A clear scale (e.g. 14 / 16 / 20 / 28 / 40 / 56 px), line-height 1.5 for body, tighter for headings, 60-75 characters per line.
- **Colour**: CSS custom properties / variables. One neutral ramp, one main accent, at most one secondary accent, plus
  semantic tokens (success, warning, error). Check contrast (minimum 4.5:1 for body text, 3:1 for large text).
  Plan and support dark mode if relevant to the product.
- **Space**: a consistent 4 or 8 px scale; generous whitespace; align components to an explicit grid.
  Consistent border-radius and shadow elevations.
- **Motion**: subtle and purposeful (150-250 ms ease-out transitions for hovers and reveals); always respect `prefers-reduced-motion: reduce`.

## 3. Avoid the generic look
- No default purple-to-blue gradient hero with three identical cards underneath.
- Vary rhythm: mix full-width sections, asymmetric layouts, and dense detail areas.
- Real, specific copy instead of "Lorem ipsum" or vague slogans; realistic, plausible sample data in dashboards and tables.
- Complete state coverage: give interactive components default, hover, visible focus (`:focus-visible`), active, disabled, loading, empty, and error states.

## 4. Build it properly
- Semantic HTML (`header`, `nav`, `main`, `aside`, `footer`, `button` for actions, `a` for navigation, `<label>` on every input).
- Full keyboard accessibility: tab order must follow visual layout; interactive elements must have visible focus rings.
- Mobile first; verify responsive breakpoints at 375 px (mobile), 768 px (tablet), and 1280+ px (desktop). Use CSS Flexbox and Grid; avoid fixed pixel page containers.
- Local icons and media: use inline SVGs or local assets rather than external CDN icon fonts.
- Images: descriptive `alt` attributes, explicit aspect ratios/dimensions, and `loading="lazy"` below the fold.

## 5. Check
Open the interface in a browser (or local preview server), test each responsive width, cycle light/dark themes,
tab through with the keyboard, and fix overflow, misalignment, and contrast issues before declaring done.
