"""Small helper for PDFs: info, text, merge, split, pages, rotate. Requires pypdf."""
import os
import sys

try:
    from pypdf import PdfReader, PdfWriter
except ImportError:
    sys.exit("pypdf is not installed. Run: python -m pip install pypdf")


def page_list(spec, n):
    if not spec:
        return list(range(n))
    out = []
    for part in spec.split(','):
        if '-' in part:
            a, b = part.split('-')
            out.extend(range(int(a) - 1, min(int(b), n)))
        else:
            out.append(int(part) - 1)
    return [p for p in out if 0 <= p < n]


def info(path):
    r = PdfReader(path)
    meta = r.metadata or {}
    print(f'pages: {len(r.pages)}')
    print(f'encrypted: {r.is_encrypted}')
    if meta.get('/Title'):
        print(f'title: {meta.get("/Title")}')


def text(path, spec=None):
    r = PdfReader(path)
    for i in page_list(spec, len(r.pages)):
        t = r.pages[i].extract_text() or ''
        print(f'--- page {i + 1} ---')
        print(t.strip() if t.strip() else '[no text on this page - may be a scanned image]')


def write(writer, out):
    with open(out, 'wb') as f:
        writer.write(f)
    print(f'Wrote {out} ({len(writer.pages)} pages)')


def merge(out, *inputs):
    w = PdfWriter()
    for p in inputs:
        for page in PdfReader(p).pages:
            w.add_page(page)
    write(w, out)


def split(path, folder):
    os.makedirs(folder, exist_ok=True)
    r = PdfReader(path)
    base = os.path.splitext(os.path.basename(path))[0]
    for i, page in enumerate(r.pages, start=1):
        w = PdfWriter()
        w.add_page(page)
        with open(os.path.join(folder, f'{base}_p{i}.pdf'), 'wb') as f:
            w.write(f)
    print(f'Wrote {len(r.pages)} files to {folder}')


def pages(path, out, spec):
    r = PdfReader(path)
    w = PdfWriter()
    for i in page_list(spec, len(r.pages)):
        w.add_page(r.pages[i])
    write(w, out)


def rotate(path, out, degrees):
    r = PdfReader(path)
    w = PdfWriter()
    for page in r.pages:
        w.add_page(page.rotate(int(degrees)))
    write(w, out)


if __name__ == '__main__':
    a = sys.argv[1:]
    cmds = {'info': (info, 1, 1), 'text': (text, 1, 2), 'merge': (merge, 2, 999), 'split': (split, 2, 2),
            'pages': (pages, 3, 3), 'rotate': (rotate, 3, 3)}
    if not a or a[0] not in cmds or not (cmds[a[0]][1] <= len(a) - 1 <= cmds[a[0]][2]):
        sys.exit('usage: pdf_tool.py info F | text F [1-3,5] | merge OUT A B.. | split F DIR | pages F OUT 2-4 | rotate F OUT 90')
    cmds[a[0]][0](*a[1:])
