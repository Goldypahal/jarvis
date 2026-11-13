//! Forgiving text replacement for `edit_file`.
//!
//! Free models often copy text slightly wrong: trailing spaces, different
//! indentation, LF vs CRLF, or the `   12\t` line-number prefix from
//! `read_file` output. When the exact text is not found, this module retries
//! line by line with looser comparisons, and only edits when exactly one place
//! in the file matches.

#[derive(Debug, PartialEq, Eq)]
pub enum FuzzyError {
    NotFound,
    Ambiguous(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub struct FuzzyEdit {
    pub text: String,
    /// 1-based line where the replaced block starts.
    pub line: usize,
    /// What had to be ignored to find the match, for the tool result.
    pub note: &'static str,
}

struct Line<'a> {
    start: usize,
    /// Content without the line ending.
    body: &'a str,
}

fn split_lines(s: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    for chunk in s.split_inclusive('\n') {
        let body = chunk.strip_suffix('\n').unwrap_or(chunk);
        let body = body.strip_suffix('\r').unwrap_or(body);
        out.push(Line { start, body });
        start += chunk.len();
    }
    out
}

/// Remove `read_file` line-number prefixes ("    12\t") if every non-empty line has one.
pub fn strip_line_numbers(s: &str) -> Option<String> {
    fn strip(line: &str) -> Option<&str> {
        let t = line.trim_start_matches(' ');
        let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 {
            return None;
        }
        t[digits..].strip_prefix('\t')
    }
    let lines: Vec<&str> = s.lines().collect();
    if lines.iter().all(|l| l.trim().is_empty()) {
        return None;
    }
    let mut out = Vec::with_capacity(lines.len());
    for l in &lines {
        if l.trim().is_empty() {
            out.push("");
        } else {
            out.push(strip(l)?);
        }
    }
    Some(out.join("\n"))
}

/// Lines of `s` with leading and trailing blank lines removed.
fn content_lines(s: &str) -> Vec<&str> {
    let mut v: Vec<&str> = s
        .lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    while v.first().is_some_and(|l| l.trim().is_empty()) {
        v.remove(0);
    }
    while v.last().is_some_and(|l| l.trim().is_empty()) {
        v.pop();
    }
    v
}

fn indent_of(s: &str) -> &str {
    &s[..s.len() - s.trim_start().len()]
}

#[derive(Clone, Copy)]
enum Mode {
    TrailingSpace,
    AllSpace,
}

fn find(file: &[Line<'_>], old: &[&str], mode: Mode) -> Vec<usize> {
    if old.is_empty() || old.len() > file.len() {
        return vec![];
    }
    let eq = |a: &str, b: &str| match mode {
        Mode::TrailingSpace => a.trim_end() == b.trim_end(),
        Mode::AllSpace => a.trim() == b.trim(),
    };
    (0..=file.len() - old.len())
        .filter(|&i| old.iter().enumerate().all(|(j, o)| eq(file[i + j].body, o)))
        .collect()
}

pub fn fuzzy_replace(original: &str, old: &str, new: &str) -> Result<FuzzyEdit, FuzzyError> {
    let eol = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let file = split_lines(original);

    // Candidates: as given, then with line-number prefixes removed.
    let mut attempts: Vec<(String, String, bool)> = vec![(old.to_string(), new.to_string(), false)];
    if let Some(o) = strip_line_numbers(old) {
        let n = strip_line_numbers(new).unwrap_or_else(|| new.to_string());
        attempts.push((o, n, true));
    }

    let mut ambiguous = None;
    for (old_s, new_s, numbered) in &attempts {
        let old_lines = content_lines(old_s);
        for mode in [Mode::TrailingSpace, Mode::AllSpace] {
            let hits = find(&file, &old_lines, mode);
            match hits.len() {
                0 => continue,
                1 => {}
                n => {
                    ambiguous.get_or_insert(n);
                    continue;
                }
            }
            let i = hits[0];
            let last = &file[i + old_lines.len() - 1];
            let (from, to) = (file[i].start, last.start + last.body.len());

            let mut new_lines: Vec<String> = content_lines(new_s)
                .into_iter()
                .map(str::to_string)
                .collect();
            if matches!(mode, Mode::AllSpace) {
                // Shift the replacement by the indentation difference the model got wrong.
                let file_indent = indent_of(file[i].body);
                let model_indent = indent_of(old_lines[0]);
                for l in &mut new_lines {
                    if let Some(rest) = l.strip_prefix(model_indent) {
                        *l = format!("{file_indent}{rest}");
                    }
                }
            }
            let replacement = new_lines.join(eol);
            let text = format!("{}{}{}", &original[..from], replacement, &original[to..]);
            let note = match (numbered, mode) {
                (false, Mode::TrailingSpace) => "matched ignoring trailing spaces and line endings",
                (false, Mode::AllSpace) => "matched ignoring indentation; replacement re-indented",
                (true, Mode::TrailingSpace) => "matched after removing line-number prefixes",
                (true, Mode::AllSpace) => {
                    "matched after removing line-number prefixes and ignoring indentation"
                }
            };
            return Ok(FuzzyEdit {
                text,
                line: i + 1,
                note,
            });
        }
    }
    Err(match ambiguous {
        Some(n) => FuzzyError::Ambiguous(n),
        None => FuzzyError::NotFound,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_spaces_and_crlf() {
        let file = "def add(a, b):  \r\n    return a - b\r\n";
        let e = fuzzy_replace(
            file,
            "def add(a, b):\n    return a - b",
            "def add(a, b):\n    return a + b",
        )
        .unwrap();
        assert_eq!(e.text, "def add(a, b):\r\n    return a + b\r\n");
        assert_eq!(e.line, 1);
    }

    #[test]
    fn wrong_indentation_is_reindented() {
        let file = "class A:\n    def f(self):\n        return 1\n";
        let e = fuzzy_replace(
            file,
            "def f(self):\n    return 1",
            "def f(self):\n    return 2",
        )
        .unwrap();
        assert_eq!(e.text, "class A:\n    def f(self):\n        return 2\n");
        assert_eq!(e.line, 2);
        assert!(e.note.contains("indentation"));
    }

    #[test]
    fn line_number_prefixes_removed() {
        let file = "a\nb = 1\nc\n";
        let e = fuzzy_replace(file, "     2\tb = 1", "     2\tb = 2").unwrap();
        assert_eq!(e.text, "a\nb = 2\nc\n");
        assert!(e.note.contains("line-number"));
    }

    #[test]
    fn ambiguous_and_missing() {
        let file = "x = 1 \nx = 1\n";
        assert_eq!(
            fuzzy_replace(file, "x = 1  ", "y"),
            Err(FuzzyError::Ambiguous(2))
        );
        assert_eq!(fuzzy_replace(file, "zzz", "y"), Err(FuzzyError::NotFound));
    }

    #[test]
    fn leading_blank_lines_ignored() {
        let file = "a\nb\n";
        let e = fuzzy_replace(file, "\nb \n", "B").unwrap();
        assert_eq!(e.text, "a\nB\n");
    }
}
