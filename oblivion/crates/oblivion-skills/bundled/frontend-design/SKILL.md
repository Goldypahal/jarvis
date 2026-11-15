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
- **Type**: two families at most. Pick characterful fonts that suit the direction (from Google Fonts or a
  system stack), not the same default every time. A clear scale (e.g. 14 / 16 / 20 / 28 / 40 / 56 px),
  line-height 1.5 for body, tighter for headings, 60-75 characters per line.
- **Colour**: CSS variables. One neutral ramp, one main accent, at most one secondary accent, plus
  success/warning/error. Check contrast (4.5:1 for body text). Plan dark mode if it fits the product.
- **Space**: a 4 or 8 px scale; generous whitespace; align to a grid. Consistent radius and shadow levels.
- **Motion**: subtle and purposeful (150-250 ms ease-out for hovers and reveals); respect `prefers-reduced-motion`.

## 3. Avoid the generic look
- No default purple-to-blue gradient hero with three identical cards underneath.
- Vary rhythm: mix full-width sections, asymmetric layouts and dense detail areas.
- Real, specific copy instead of "Lorem ipsum" or vague slogans; realistic sample data in dashboards.
- Give components states: hover, focus (visible outline), active, disabled, loading, empty, error.

## 4. Build it properly
- Semantic HTML (`header`, `nav`, `main`, `button` for actions, `a` for navigation, labels on every input).
- Mobile first; test at 375, 768 and 1280 px widths. Flexbox/Grid, no fixed pixel page widths.
- Use the project's stack and component library if it has one; otherwise plain HTML/CSS or the framework asked for.
- Images: descriptive `alt`, explicit width/height, lazy loading below the fold. Icons from one set.

## 5. Check
Open it (a local server or file), look at each breakpoint, tab through with the keyboard, and fix
overflow, misalignment and contrast problems before calling it done. Summarise the direction and how to run it.
