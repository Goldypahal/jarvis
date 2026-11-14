"""Small helper for Word files: read, from-md, replace. Requires python-docx."""
import re
import sys

try:
    import docx
    from docx.shared import Pt  # noqa: F401
except ImportError:
    sys.exit("python-docx is not installed. Run: python -m pip install python-docx")


def read(path):
    d = docx.Document(path)
    body = d.element.body
    for child in body.iterchildren():
        tag = child.tag.split('}')[-1]
        if tag == 'p':
            p = docx.text.paragraph.Paragraph(child, d)
            style = (p.style.name or '').lower() if p.style is not None else ''
            text = p.text
            if not text.strip():
                continue
            m = re.match(r'heading (\d)', style)
            if m:
                print('#' * int(m.group(1)) + ' ' + text)
            elif 'title' == style:
                print('# ' + text)
            elif 'list number' in style:
                print('1. ' + text)
            elif 'list' in style:
                print('- ' + text)
            else:
                print(text)
            print()
        elif tag == 'tbl':
            t = docx.table.Table(child, d)
            rows = [[c.text.strip().replace('\n', ' ') for c in r.cells] for r in t.rows]
            if rows:
                print('| ' + ' | '.join(rows[0]) + ' |')
                print('|' + '---|' * len(rows[0]))
                for r in rows[1:]:
                    print('| ' + ' | '.join(r) + ' |')
                print()


def add_inline(par, text):
    # **bold** and *italic*
    for part in re.split(r'(\*\*[^*]+\*\*|\*[^*]+\*)', text):
        if not part:
            continue
        if part.startswith('**') and part.endswith('**'):
            par.add_run(part[2:-2]).bold = True
        elif part.startswith('*') and part.endswith('*') and len(part) > 2:
            par.add_run(part[1:-1]).italic = True
        else:
            par.add_run(part)


def from_md(src, out, title=None):
    lines = open(src, encoding='utf-8').read().splitlines()
    d = docx.Document()
    if title:
        d.core_properties.title = title
    i = 0
    while i < len(lines):
        line = lines[i].rstrip()
        if not line.strip():
            i += 1
            continue
        m = re.match(r'^(#{1,4})\s+(.*)', line)
        if m:
            d.add_heading(m.group(2).strip(), level=len(m.group(1)))
            i += 1
            continue
        if line.lstrip().startswith('|'):
            block = []
            while i < len(lines) and lines[i].lstrip().startswith('|'):
                block.append(lines[i].strip())
                i += 1
            rows = [[c.strip() for c in r.strip('|').split('|')] for r in block if not re.match(r'^\|?\s*:?-{2,}', r)]
            if rows:
                t = d.add_table(rows=len(rows), cols=max(len(r) for r in rows))
                t.style = 'Table Grid'
                for ri, r in enumerate(rows):
                    for ci, val in enumerate(r):
                        cell = t.cell(ri, ci)
                        cell.text = ''
                        add_inline(cell.paragraphs[0], val)
                        if ri == 0:
                            for run in cell.paragraphs[0].runs:
                                run.bold = True
            continue
        m = re.match(r'^\s*[-*]\s+(.*)', line)
        if m:
            add_inline(d.add_paragraph(style='List Bullet'), m.group(1))
            i += 1
            continue
        m = re.match(r'^\s*\d+[.)]\s+(.*)', line)
        if m:
            add_inline(d.add_paragraph(style='List Number'), m.group(1))
            i += 1
            continue
        para = [line.strip()]
        i += 1
        while i < len(lines) and lines[i].strip() and not re.match(r'^(#{1,4}\s|\s*[-*]\s|\s*\d+[.)]\s|\s*\|)', lines[i]):
            para.append(lines[i].strip())
            i += 1
        add_inline(d.add_paragraph(), ' '.join(para))
    d.save(out)
    print(f'Wrote {out}')


def replace(src, out, old, new):
    d = docx.Document(src)
    n = 0
    def fix(paragraphs):
        nonlocal n
        for p in paragraphs:
            for r in p.runs:
                if old in r.text:
                    n += r.text.count(old)
                    r.text = r.text.replace(old, new)
    fix(d.paragraphs)
    for t in d.tables:
        for row in t.rows:
            for c in row.cells:
                fix(c.paragraphs)
    d.save(out)
    print(f'Replaced {n} occurrence(s); wrote {out}')


if __name__ == '__main__':
    a = sys.argv[1:]
    if len(a) >= 2 and a[0] == 'read':
        read(a[1])
    elif len(a) >= 3 and a[0] == 'from-md':
        title = a[a.index('--title') + 1] if '--title' in a else None
        from_md(a[1], a[2], title)
    elif len(a) == 5 and a[0] == 'replace':
        replace(*a[1:])
    else:
        sys.exit('usage: docx_tool.py read FILE | from-md IN.md OUT.docx [--title T] | replace IN OUT OLD NEW')
