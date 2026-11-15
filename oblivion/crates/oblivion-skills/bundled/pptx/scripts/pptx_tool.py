"""Helper for PowerPoint files: read, from-md, check, replace. Requires python-pptx.

Markdown slide format for from-md (one slide per `# ` heading):

    # Quarterly review            <- first slide = title slide
    Sales and hiring, Q3 2026     <- its subtitle

    # Highlights                  <- a normal slide
    - Revenue up **18%**          <- bullets (indent two spaces for a sub-bullet)
      - Mostly from renewals
    - Two new hires
    > Speaker notes go on lines starting with "> "

    # Revenue by region
    | Region | Q2 | Q3 |          <- a table
    |---|---|---|
    | North | 1.2 | 1.4 |

    # Our new office
    ![Office photo](office.jpg)   <- a picture (beside bullets if the slide has any)

    # Questions?                  <- a slide with no body = big section/closing slide
"""
import argparse
import os
import re
import sys

try:
    from pptx import Presentation
    from pptx.dml.color import RGBColor
    from pptx.enum.text import PP_ALIGN, MSO_ANCHOR
    from pptx.oxml.ns import qn
    from pptx.util import Emu, Inches, Pt
except ImportError:
    sys.exit("python-pptx is not installed. Run: python -m pip install python-pptx")

THEMES = {
    # background, title text, body text, accent, muted
    "light": ("FFFFFF", "1F2937", "374151", "2563EB", "6B7280"),
    "dark": ("111827", "F9FAFB", "E5E7EB", "38BDF8", "9CA3AF"),
    "ocean": ("F0F7FF", "0B3B60", "1E3A5F", "0EA5E9", "5B7A99"),
    "forest": ("F4F8F2", "1F3B2D", "2F4A3A", "2E9E5B", "6B8577"),
    "sunset": ("FFF8F1", "4A1D0F", "5C2E1E", "EA580C", "9A6B57"),
    "mono": ("FAFAFA", "111111", "333333", "111111", "777777"),
}
W, H = Inches(13.333), Inches(7.5)
MARGIN = Inches(0.7)
FONT = "Calibri"


def rgb(hexstr):
    return RGBColor.from_string(hexstr)


# ---------- read ----------

def shape_text(shape):
    lines = []
    if shape.has_text_frame:
        paras = [p for p in shape.text_frame.paragraphs if "".join(r.text for r in p.runs).strip()]
        for p in paras:
            t = "".join(r.text for r in p.runs).strip()
            bulleted = p._p.pPr is not None and p._p.pPr.find(qn("a:buChar")) is not None
            if bulleted or p.level:
                lines.append("  " * p.level + "- " + t)
            else:
                lines.append(t)
    return lines


def read(path):
    prs = Presentation(path)
    print(f"{len(prs.slides)} slides, {Emu(prs.slide_width).inches:.2f} x {Emu(prs.slide_height).inches:.2f} in\n")
    for i, slide in enumerate(prs.slides, 1):
        title = slide.shapes.title.text.strip() if slide.shapes.title is not None and slide.shapes.title.has_text_frame else ""
        print(f"## Slide {i}: {title or '(no title)'}")
        pictures = 0
        for shape in slide.shapes:
            if slide.shapes.title is not None and shape.shape_id == slide.shapes.title.shape_id:
                continue
            if shape.has_table:
                rows = [[c.text.strip().replace("\n", " ") for c in r.cells] for r in shape.table.rows]
                if rows:
                    print("| " + " | ".join(rows[0]) + " |")
                    print("|" + "---|" * len(rows[0]))
                    for r in rows[1:]:
                        print("| " + " | ".join(r) + " |")
            elif shape.shape_type == 13:  # picture
                pictures += 1
            else:
                for line in shape_text(shape):
                    print(line)
        if pictures:
            print(f"[{pictures} picture(s)]")
        if slide.has_notes_slide:
            notes = slide.notes_slide.notes_text_frame.text.strip()
            if notes:
                print("> " + notes.replace("\n", "\n> "))
        print()


# ---------- markdown -> slides ----------

def parse_md(text):
    slides, cur = [], None
    for raw in text.splitlines():
        line = raw.rstrip()
        if line.startswith("# "):
            cur = {"title": line[2:].strip(), "items": [], "notes": [], "table": [], "images": [], "paras": []}
            slides.append(cur)
            continue
        if cur is None or not line.strip():
            continue
        s = line.strip()
        if s.startswith("> "):
            cur["notes"].append(s[2:])
        elif s.startswith("|"):
            cells = [c.strip() for c in s.strip("|").split("|")]
            if not all(re.fullmatch(r":?-{2,}:?", c) for c in cells):
                cur["table"].append(cells)
        elif m := re.match(r"!\[([^\]]*)\]\(([^)]+)\)", s):
            cur["images"].append(m.group(2))
        elif m := re.match(r"^(\s*)([-*+]|\d+[.)])\s+(.*)", line):
            level = min(len(m.group(1).replace("\t", "  ")) // 2, 2)
            prefix = m.group(2) + " " if m.group(2)[0].isdigit() else ""
            cur["items"].append((level, prefix + m.group(3)))
        elif s.startswith("## "):
            cur["paras"].append(s[3:])
        else:
            cur["paras"].append(s)
    return slides


def add_runs(paragraph, text, size, color, bold=False):
    for part in re.split(r"(\*\*[^*]+\*\*)", text):
        if not part:
            continue
        run = paragraph.add_run()
        is_bold = part.startswith("**") and part.endswith("**")
        run.text = part[2:-2] if is_bold else part
        f = run.font
        f.name, f.size, f.color.rgb, f.bold = FONT, Pt(size), rgb(color), bold or is_bold


def textbox(slide, x, y, w, h, anchor=MSO_ANCHOR.TOP):
    tb = slide.shapes.add_textbox(x, y, w, h)
    tf = tb.text_frame
    tf.word_wrap = True
    tf.vertical_anchor = anchor
    tf.margin_left = tf.margin_right = Inches(0.05)
    return tf


def background(slide, theme):
    fill = slide.background.fill
    fill.solid()
    fill.fore_color.rgb = rgb(theme[0])


def accent_bar(slide, theme, x, y, w, h):
    bar = slide.shapes.add_shape(1, x, y, w, h)  # rectangle
    bar.fill.solid()
    bar.fill.fore_color.rgb = rgb(theme[3])
    bar.line.fill.background()
    return bar


def body_size(items, paras):
    words = sum(len(t.split()) for _, t in items) + sum(len(p.split()) for p in paras)
    n = len(items) + len(paras)
    if n <= 4 and words <= 40:
        return 26
    if n <= 6 and words <= 70:
        return 22
    return 18


def style_title(shape, x, y, w, h, text, size, color, align=None, anchor=MSO_ANCHOR.MIDDLE):
    shape.left, shape.top, shape.width, shape.height = x, y, w, h
    tf = shape.text_frame
    tf.clear()
    tf.word_wrap = True
    tf.vertical_anchor = anchor
    p = tf.paragraphs[0]
    if align is not None:
        p.alignment = align
    add_runs(p, text, size, color, bold=True)


def title_slide(prs, s, theme):
    slide = prs.slides.add_slide(prs.slide_layouts[0])  # Title Slide
    background(slide, theme)
    style_title(slide.shapes.title, MARGIN, Inches(1.6), W - 2 * MARGIN, Inches(1.9), s["title"], 48,
                theme[1], PP_ALIGN.LEFT, MSO_ANCHOR.BOTTOM)
    accent_bar(slide, theme, MARGIN, Inches(3.68), Inches(1.2), Inches(0.08))
    sub = " ".join(s["paras"]) or " ".join(t for _, t in s["items"])
    subtitle = slide.placeholders[1]
    if sub:
        subtitle.left, subtitle.top = MARGIN, Inches(4.0)
        subtitle.width, subtitle.height = W - 2 * MARGIN, Inches(1.5)
        tf = subtitle.text_frame
        tf.clear()
        tf.word_wrap = True
        tf.vertical_anchor = MSO_ANCHOR.TOP
        tf.paragraphs[0].alignment = PP_ALIGN.LEFT
        add_runs(tf.paragraphs[0], sub, 22, theme[4])
    else:
        subtitle._element.getparent().remove(subtitle._element)
    return slide


def section_slide(prs, s, theme):
    slide = prs.slides.add_slide(prs.slide_layouts[5])  # Title Only
    background(slide, theme)
    style_title(slide.shapes.title, MARGIN, Inches(2.4), W - 2 * MARGIN, Inches(2.0), s["title"], 44,
                theme[1], PP_ALIGN.CENTER)
    accent_bar(slide, theme, int((W - Inches(1.2)) / 2), Inches(4.55), Inches(1.2), Inches(0.08))
    return slide


def content_slide(prs, s, theme, base_dir):
    slide = prs.slides.add_slide(prs.slide_layouts[5])  # Title Only
    background(slide, theme)
    accent_bar(slide, theme, 0, 0, Inches(0.18), H)
    style_title(slide.shapes.title, MARGIN, Inches(0.45), W - 2 * MARGIN, Inches(1.0), s["title"], 34,
                theme[1], PP_ALIGN.LEFT)
    top = Inches(1.65)
    avail_h = H - top - Inches(0.5)
    images = [os.path.join(base_dir, p) if not os.path.isabs(p) else p for p in s["images"]]
    has_text = bool(s["items"] or s["paras"])
    text_w = W - 2 * MARGIN
    if images and has_text:
        text_w = (W - 2 * MARGIN) * 0.55
    if s["table"]:
        rows, cols = len(s["table"]), max(len(r) for r in s["table"])
        table_top = top
        if has_text:
            body = textbox(slide, MARGIN, top, text_w, Inches(1.0))
            fill_body(body, s, theme, 18)
            table_top = top + Inches(1.1)
        height = min(Inches(0.6 if rows <= 5 else 0.5) * rows, H - table_top - Inches(0.4))
        shape = slide.shapes.add_table(rows, cols, MARGIN, table_top, W - 2 * MARGIN, height)
        size = 20 if rows <= 5 else 16 if rows <= 8 else 12
        for r, row in enumerate(s["table"]):
            for c in range(cols):
                cell = shape.table.cell(r, c)
                cell.text = ""
                add_runs(cell.text_frame.paragraphs[0], row[c] if c < len(row) else "", size,
                         "FFFFFF" if r == 0 else theme[2], bold=(r == 0))
                cell.fill.solid()
                cell.fill.fore_color.rgb = rgb(theme[3]) if r == 0 else rgb(theme[0])
    elif has_text:
        body = textbox(slide, MARGIN, top, text_w, avail_h)
        fill_body(body, s, theme, body_size(s["items"], s["paras"]))
    for k, img in enumerate(images[:1]):
        if not os.path.exists(img):
            print(f"warning: picture not found: {img}", file=sys.stderr)
            continue
        if has_text:
            x, w = MARGIN + text_w + Inches(0.4), (W - 2 * MARGIN) - text_w - Inches(0.4)
        else:
            x, w = MARGIN, W - 2 * MARGIN
        pic = slide.shapes.add_picture(img, x, top)
        # Fit inside the box, keeping the aspect ratio.
        scale = min(w / pic.width, avail_h / pic.height, 1.0 if not has_text else 10)
        pic.width, pic.height = int(pic.width * scale), int(pic.height * scale)
        pic.left = int(x + (w - pic.width) / 2)
    return slide


def set_bullet(p, level, color, char):
    """A real PowerPoint bullet (hanging indent, colored bullet character)."""
    p.level = level
    pPr = p._p.get_or_add_pPr()
    pPr.set("marL", str(int(Inches(0.38 + 0.45 * level))))
    pPr.set("indent", str(int(-Inches(0.32))))
    for tag in ("a:buClr", "a:buNone", "a:buChar", "a:buAutoNum"):
        for el in pPr.findall(qn(tag)):
            pPr.remove(el)
    if char is None:
        pPr.append(pPr.makeelement(qn("a:buNone"), {}))
        return
    clr = pPr.makeelement(qn("a:buClr"), {})
    clr.append(clr.makeelement(qn("a:srgbClr"), {"val": color}))
    pPr.append(clr)
    pPr.append(pPr.makeelement(qn("a:buChar"), {"char": char}))


def fill_body(tf, s, theme, size):
    first = True
    for para in s["paras"]:
        p = tf.paragraphs[0] if first else tf.add_paragraph()
        first = False
        p.space_after = Pt(size * 0.5)
        add_runs(p, para, size, theme[2])
    for level, text in s["items"]:
        p = tf.paragraphs[0] if first else tf.add_paragraph()
        first = False
        p.space_after = Pt(size * 0.45)
        numbered = bool(re.match(r"\d+[.)] ", text))
        set_bullet(p, level, theme[3], None if numbered else ("•" if level == 0 else "–"))
        add_runs(p, text, size - 2 * level, theme[2])


def from_md(src, out, theme_name):
    theme = THEMES.get(theme_name)
    if theme is None:
        sys.exit(f"unknown theme {theme_name!r}; choose from: {', '.join(THEMES)}")
    slides = parse_md(open(src, encoding="utf-8").read())
    if not slides:
        sys.exit("no slides found: start each slide with a line '# Slide title'")
    prs = Presentation()
    prs.slide_width, prs.slide_height = W, H
    base_dir = os.path.dirname(os.path.abspath(src))
    for i, s in enumerate(slides):
        empty = not (s["items"] or s["paras"] or s["table"] or s["images"])
        if i == 0:
            slide = title_slide(prs, s, theme)
        elif empty:
            slide = section_slide(prs, s, theme)
        else:
            slide = content_slide(prs, s, theme, base_dir)
        if s["notes"]:
            slide.notes_slide.notes_text_frame.text = "\n".join(s["notes"])
    prs.save(out)
    print(f"wrote {out}: {len(slides)} slides, theme {theme_name}")
    problems = check_slides(slides)
    for p in problems:
        print("warning: " + p)


# ---------- check ----------

def check_slides(slides):
    problems = []
    for i, s in enumerate(slides, 1):
        words = sum(len(t.split()) for _, t in s["items"]) + sum(len(p.split()) for p in s["paras"])
        lines = len(s["items"]) + len(s["paras"])
        if len(s["title"]) > 60:
            problems.append(f"slide {i}: title is {len(s['title'])} characters; keep it under 60")
        if lines > 7:
            problems.append(f"slide {i}: {lines} bullets/paragraphs; split into two slides (max ~6)")
        if words > 90:
            problems.append(f"slide {i}: {words} words; too dense, move detail to speaker notes")
        if len(s["table"]) > 10:
            problems.append(f"slide {i}: table has {len(s['table'])} rows; keep tables to ~8 rows")
    return problems


def check(path):
    if path.lower().endswith(".md"):
        problems = check_slides(parse_md(open(path, encoding="utf-8").read()))
    else:
        prs = Presentation(path)
        slides = []
        for slide in prs.slides:
            title = slide.shapes.title.text if slide.shapes.title is not None else ""
            items = []
            for shape in slide.shapes:
                if shape.has_text_frame:
                    for p in shape.text_frame.paragraphs:
                        t = "".join(r.text for r in p.runs).strip()
                        if t and t != title:
                            items.append((0, t))
            if not title and items:
                title = items.pop(0)[1]
            slides.append({"title": title, "items": items, "paras": [], "table": []})
        problems = check_slides(slides)
    print("\n".join(problems) if problems else "no problems found")


# ---------- replace ----------

def replace(src, out, old, new):
    prs = Presentation(src)
    count = 0
    for slide in prs.slides:
        for shape in slide.shapes:
            frames = []
            if shape.has_text_frame:
                frames.append(shape.text_frame)
            if shape.has_table:
                frames += [c.text_frame for r in shape.table.rows for c in r.cells]
            for tf in frames:
                for p in tf.paragraphs:
                    for r in p.runs:
                        if old in r.text:
                            count += r.text.count(old)
                            r.text = r.text.replace(old, new)
    prs.save(out)
    print(f"replaced {count} occurrence(s); wrote {out}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("read").add_argument("path")
    f = sub.add_parser("from-md")
    f.add_argument("src")
    f.add_argument("out")
    f.add_argument("--theme", default="light", help="one of: " + ", ".join(THEMES))
    sub.add_parser("check").add_argument("path")
    r = sub.add_parser("replace")
    for a in ("src", "out", "old", "new"):
        r.add_argument(a)
    sub.add_parser("themes")
    a = ap.parse_args()
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(errors="replace")  # Windows consoles without UTF-8
    if a.cmd == "read":
        read(a.path)
    elif a.cmd == "from-md":
        from_md(a.src, a.out, a.theme)
    elif a.cmd == "check":
        check(a.path)
    elif a.cmd == "replace":
        replace(a.src, a.out, a.old, a.new)
    else:
        print("\n".join(f"{k}: background #{v[0]}, accent #{v[3]}" for k, v in THEMES.items()))


if __name__ == "__main__":
    main()
