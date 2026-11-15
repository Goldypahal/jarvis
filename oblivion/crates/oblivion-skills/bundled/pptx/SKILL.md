---
name: pptx
description: Create, read or edit PowerPoint presentations (.pptx) - slide decks, pitch decks, talk slides. Use when the user asks for a presentation, slides, a deck, a .pptx file, or wants to read or change an existing .pptx.
license: MIT
---
# PowerPoint presentations (.pptx)

Needs Python with python-pptx: `python -m pip install python-pptx` (run once if the script says it is missing).
The script is `scripts/pptx_tool.py` in this skill's base folder; call it with its full path.

Read a presentation
- `python <base>/scripts/pptx_tool.py read deck.pptx` prints every slide's title, bullets, tables and speaker notes.

Create a new presentation
1. Plan the story first: who the audience is, the one message, then 6–12 slides (title, agenda or problem,
   3–8 content slides, summary or next steps, closing). One idea per slide.
2. Write the slides as Markdown in a file (e.g. `slides.md`):
   - `# Title` starts a slide. The FIRST slide is the title slide; the line under it is the subtitle.
   - `- ` bullets, indent two spaces for a sub-bullet; `**bold**` for key numbers. Max ~6 bullets,
     ~12 words each. Put detail in speaker notes, not on the slide.
   - `> ` lines are speaker notes.
   - Pipe tables (`| a | b |` + `|---|---|`) for comparisons; keep to ~8 rows.
   - `![caption](picture.png)` adds a picture (path relative to the Markdown file); with bullets it sits on the right.
   - A slide with only a title becomes a big section/closing slide (e.g. `# Questions?`).
3. Build it: `python <base>/scripts/pptx_tool.py from-md slides.md deck.pptx --theme light`
   Themes: light, dark, ocean, forest, sunset, mono (`themes` lists them). Pick one that suits the topic,
   or ask the user if they care.
4. Check it: the build prints warnings for crowded slides; fix them (split slides, shorten text) and rebuild.
   Then `read deck.pptx` and compare with the plan.
5. PDF copy if asked: `soffice --headless --convert-to pdf deck.pptx` (needs LibreOffice), or tell the
   user to use File > Export in PowerPoint.

Edit an existing presentation
1. `read` it first. Save to a new file unless the user asked to overwrite.
2. Text replacement that keeps formatting:
   `python <base>/scripts/pptx_tool.py replace in.pptx out.pptx "old text" "new text"`
   (reports how many places changed; if 0, the text is split across formatting runs).
3. For anything else write a short python-pptx script (Presentation(path), .slides, .shapes,
   shape.text_frame, .slides.add_slide(prs.slide_layouts[5]), .save) and run it.
4. `read` the result and `check` it: `python <base>/scripts/pptx_tool.py check out.pptx`.

Writing good slides
- Titles say the takeaway ("Costs fell 18% after the switch"), not just the topic ("Costs").
- Numbers beat adjectives. Cite sources for facts in the notes or a final Sources slide.
- Do not invent data, quotes or logos. If you do not have a picture, do not reference one.
