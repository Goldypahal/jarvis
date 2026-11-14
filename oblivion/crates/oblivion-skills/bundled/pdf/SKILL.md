---
name: pdf
description: Read, extract text from, merge, split, rotate or create PDF files. Use when the user gives a PDF to read or summarize, wants PDFs combined or split, or asks for a PDF output.
license: MIT
---
# PDF files

Needs Python with pypdf: `python -m pip install pypdf` (once, if missing).
Helper: `scripts/pdf_tool.py` in this skill's base folder.

Read / summarize
1. `python <base>/scripts/pdf_tool.py info file.pdf` (pages, title, encrypted?).
2. `python <base>/scripts/pdf_tool.py text file.pdf 1-5` prints the text of pages 1–5 (omit the range for all). Read long PDFs in chunks of ~10 pages.
3. If a page has no text, it is probably scanned: tell the user OCR is needed (e.g. install Tesseract) rather than guessing.

Change PDFs
- Merge: `python <base>/scripts/pdf_tool.py merge out.pdf a.pdf b.pdf ...`
- Split into single pages: `python <base>/scripts/pdf_tool.py split in.pdf out_folder`
- Extract pages: `python <base>/scripts/pdf_tool.py pages in.pdf out.pdf 2-4,7`
- Rotate: `python <base>/scripts/pdf_tool.py rotate in.pdf out.pdf 90`
Always run `info` on the output to confirm the page count.

Create a PDF
- Best: make a .docx with the docx skill, then convert with LibreOffice if installed:
  `soffice --headless --convert-to pdf file.docx`
- Otherwise write HTML and tell the user to print it to PDF from a browser, or use the `reportlab` package for simple documents.
