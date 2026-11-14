---
name: docx
description: Create, read or edit Word documents (.docx). Use when the user asks for a Word file, a .docx report, letter or resume, or wants to read or change an existing .docx.
license: MIT
---
# Word documents (.docx)

Needs Python with python-docx: `python -m pip install python-docx` (run once if the script says it is missing).
The script is `scripts/docx_tool.py` in this skill's base folder; call it with its full path.

Read a document
- `python <base>/scripts/docx_tool.py read input.docx` prints headings, paragraphs and tables as Markdown.

Create a new document
1. Write the content as Markdown in a file (e.g. `draft.md`). Supported: `#`–`####` headings, paragraphs, `- ` bullets, `1. ` numbered items, `**bold**`, `*italic*`, and pipe tables (`| a | b |` with a `|---|---|` line).
2. `python <base>/scripts/docx_tool.py from-md draft.md output.docx --title "Document title"`
3. Check it: `python <base>/scripts/docx_tool.py read output.docx` and compare with the draft.

Edit an existing document
1. Read it first (above). Copy it before editing if the user did not ask to overwrite.
2. For text replacement keep formatting: `python <base>/scripts/docx_tool.py replace input.docx output.docx "old text" "new text"` (works within single runs; it reports how many places changed — if 0, the text is split across formatting runs: rewrite that paragraph with python-docx).
3. For anything else, write a short python-docx script (Document(path), .paragraphs, .tables, .add_paragraph, .save) and run it.
4. Read the result back to verify.
