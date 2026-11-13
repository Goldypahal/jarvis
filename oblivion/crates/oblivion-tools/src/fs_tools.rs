use oblivion_providers::ToolSpec;
use serde_json::{json, Value};

use crate::{str_arg, ToolBox};

const DEFAULT_LINE_LIMIT: usize = 2000;
const MAX_LINE_CHARS: usize = 2000;

pub(crate) fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "read_file".into(),
            description: "Read a text file and return it with line numbers. Use offset/limit for large \
                files. If the path is a directory, lists its entries instead. Always read a file \
                before editing it."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File or directory path, relative to the workspace or absolute"},
                    "offset": {"type": "integer", "description": "1-based line to start from (default 1)"},
                    "limit": {"type": "integer", "description": "Max lines to return (default 2000)"}
                },
                "required": ["path"]
            }),
        },
        ToolSpec {
            name: "write_file".into(),
            description: "Create a file or overwrite it completely with new content. Parent folders \
                are created. Prefer edit_file for changes to existing files."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string", "description": "The full file content"}
                },
                "required": ["path", "content"]
            }),
        },
        ToolSpec {
            name: "edit_file".into(),
            description: "Replace an exact piece of text in a file. old_string must match the file \
                exactly (including indentation) and appear exactly once, unless replace_all is true. \
                Include enough surrounding lines to make it unique."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "old_string": {"type": "string", "description": "Exact text to replace"},
                    "new_string": {"type": "string", "description": "Replacement text"},
                    "replace_all": {"type": "boolean", "description": "Replace every occurrence (default false)"}
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
    ]
}

pub(crate) async fn read_file(tb: &ToolBox, args: &Value) -> Result<String, String> {
    let path = tb.resolve(str_arg(args, "path")?);
    let meta = tokio::fs::metadata(&path)
        .await
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;

    if meta.is_dir() {
        let mut entries = tokio::fs::read_dir(&path)
            .await
            .map_err(|e| e.to_string())?;
        let mut names = Vec::new();
        while let Some(e) = entries.next_entry().await.map_err(|e| e.to_string())? {
            let is_dir = e.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            let mut n = e.file_name().to_string_lossy().into_owned();
            if is_dir {
                n.push('/');
            }
            names.push(n);
        }
        names.sort();
        return Ok(format!(
            "Directory {} ({} entries):\n{}",
            path.display(),
            names.len(),
            names.join("\n")
        ));
    }

    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Err(format!(
            "{} looks like a binary file ({} bytes); use a skill or shell tool to inspect it",
            path.display(),
            bytes.len()
        ));
    }
    let text = String::from_utf8_lossy(&bytes);
    let offset = args
        .get("offset")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_LINE_LIMIT);

    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Ok(format!("{} is empty", path.display()));
    }
    if offset > lines.len() {
        return Err(format!(
            "offset {offset} is past the end of the file ({} lines)",
            lines.len()
        ));
    }
    let end = (offset - 1 + limit).min(lines.len());
    let mut out = String::new();
    for (i, line) in lines[offset - 1..end].iter().enumerate() {
        let line: String = if line.chars().count() > MAX_LINE_CHARS {
            line.chars().take(MAX_LINE_CHARS).collect::<String>() + "…"
        } else {
            (*line).to_string()
        };
        out.push_str(&format!("{:>6}\t{}\n", offset + i, line));
    }
    if end < lines.len() {
        out.push_str(&format!(
            "\n[showing lines {offset}-{end} of {}; use offset to read more]",
            lines.len()
        ));
    }
    Ok(out)
}

pub(crate) async fn write_file(tb: &ToolBox, args: &Value) -> Result<String, String> {
    let path = tb.resolve(str_arg(args, "path")?);
    let content = str_arg(args, "content")?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    tb.journal()
        .snapshot(&path)
        .map_err(|e| format!("snapshot failed, nothing written: {e}"))?;
    tokio::fs::write(&path, content)
        .await
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(format!(
        "Wrote {} ({} lines)",
        path.display(),
        content.lines().count()
    ))
}

pub(crate) async fn edit_file(tb: &ToolBox, args: &Value) -> Result<String, String> {
    let path = tb.resolve(str_arg(args, "path")?);
    let old = str_arg(args, "old_string")?;
    let new = str_arg(args, "new_string")?;
    let replace_all = args
        .get("replace_all")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if old.is_empty() {
        return Err("old_string is empty; use write_file to create a file".into());
    }
    if old == new {
        return Err("old_string and new_string are identical; nothing to change".into());
    }
    let original = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    // Models often send LF while Windows files use CRLF; match either.
    let crlf = original.contains("\r\n");
    let (old_m, new_m) = if crlf && !old.contains("\r\n") {
        (old.replace('\n', "\r\n"), new.replace('\n', "\r\n"))
    } else {
        (old.to_string(), new.to_string())
    };

    let count = original.matches(&old_m).count();
    if count == 0 {
        return Err(format!(
            "old_string was not found in {}. Read the file again and copy the text exactly, including whitespace.",
            path.display()
        ));
    }
    if count > 1 && !replace_all {
        return Err(format!(
            "old_string appears {count} times in {}. Add more surrounding lines so it is unique, or set replace_all.",
            path.display()
        ));
    }
    let updated = if replace_all {
        original.replace(&old_m, &new_m)
    } else {
        original.replacen(&old_m, &new_m, 1)
    };
    tb.journal()
        .snapshot(&path)
        .map_err(|e| format!("snapshot failed, nothing changed: {e}"))?;
    tokio::fs::write(&path, &updated)
        .await
        .map_err(|e| e.to_string())?;

    // Report where the change landed so the model can verify.
    let first_line = original[..original.find(&old_m).unwrap_or(0)]
        .lines()
        .count()
        .max(1);
    Ok(format!(
        "Edited {}: replaced {} occurrence(s), starting near line {}",
        path.display(),
        if replace_all { count } else { 1 },
        first_line
    ))
}

#[cfg(test)]
mod tests {
    use crate::{Shell, ToolBox};
    use serde_json::json;

    fn tb(dir: &std::path::Path) -> ToolBox {
        ToolBox::new(dir, Shell::Sh, dir.join(".snap"))
    }

    #[tokio::test]
    async fn write_read_edit_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let t = tb(d.path());
        let w = t
            .execute(
                "write_file",
                &json!({"path":"src/a.txt","content":"one\ntwo\nthree\n"}),
            )
            .await;
        assert!(!w.is_error, "{}", w.text);
        let r = t
            .execute(
                "read_file",
                &json!({"path":"src/a.txt","offset":2,"limit":1}),
            )
            .await;
        assert!(r.text.contains("     2\ttwo"), "{}", r.text);
        assert!(r.text.contains("showing lines 2-2 of 3"));
        let e = t
            .execute(
                "edit_file",
                &json!({"path":"src/a.txt","old_string":"two","new_string":"TWO"}),
            )
            .await;
        assert!(!e.is_error, "{}", e.text);
        assert_eq!(
            std::fs::read_to_string(d.path().join("src/a.txt")).unwrap(),
            "one\nTWO\nthree\n"
        );
    }

    #[tokio::test]
    async fn edit_rejects_ambiguous_and_missing() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("f"), "x x").unwrap();
        let t = tb(d.path());
        let e = t
            .execute(
                "edit_file",
                &json!({"path":"f","old_string":"x","new_string":"y"}),
            )
            .await;
        assert!(e.is_error && e.text.contains("appears 2 times"));
        let e = t
            .execute(
                "edit_file",
                &json!({"path":"f","old_string":"z","new_string":"y"}),
            )
            .await;
        assert!(e.is_error && e.text.contains("not found"));
        let e = t
            .execute(
                "edit_file",
                &json!({"path":"f","old_string":"x","new_string":"y","replace_all":true}),
            )
            .await;
        assert!(!e.is_error);
        assert_eq!(std::fs::read_to_string(d.path().join("f")).unwrap(), "y y");
    }

    #[tokio::test]
    async fn edit_handles_crlf_files() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("w.txt"), "a\r\nb\r\nc\r\n").unwrap();
        let t = tb(d.path());
        let e = t
            .execute(
                "edit_file",
                &json!({"path":"w.txt","old_string":"a\nb","new_string":"a\nB"}),
            )
            .await;
        assert!(!e.is_error, "{}", e.text);
        assert_eq!(
            std::fs::read_to_string(d.path().join("w.txt")).unwrap(),
            "a\r\nB\r\nc\r\n"
        );
    }

    #[tokio::test]
    async fn read_dir_and_binary() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("bin"), [0u8, 1, 2]).unwrap();
        let t = tb(d.path());
        let r = t.execute("read_file", &json!({"path":"."})).await;
        assert!(r.text.contains("sub/") && r.text.contains("bin"));
        let r = t.execute("read_file", &json!({"path":"bin"})).await;
        assert!(r.is_error && r.text.contains("binary"));
    }

    #[tokio::test]
    async fn undo_restores_files() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("keep.txt"), "original").unwrap();
        let t = tb(d.path());
        let mark = t.journal().mark();
        t.execute(
            "edit_file",
            &json!({"path":"keep.txt","old_string":"original","new_string":"changed"}),
        )
        .await;
        t.execute("write_file", &json!({"path":"new.txt","content":"hi"}))
            .await;
        let rep = t.journal().undo_to(mark).unwrap();
        assert_eq!(rep.restored, 1);
        assert_eq!(rep.removed, 1);
        assert_eq!(
            std::fs::read_to_string(d.path().join("keep.txt")).unwrap(),
            "original"
        );
        assert!(!d.path().join("new.txt").exists());
    }
}
