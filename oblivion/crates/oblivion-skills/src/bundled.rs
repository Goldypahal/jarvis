//! Skills shipped inside the binary (SRS 5). They are written to
//! `~/.oblivion/bundled-skills/` so their scripts exist as real files, and
//! refreshed whenever a new Oblivion version changes them.

use std::path::Path;

/// (path inside the skills folder, file contents)
pub const FILES: &[(&str, &str)] = &[
    (
        "browser/SKILL.md",
        include_str!("../bundled/browser/SKILL.md"),
    ),
    (
        "code-review/SKILL.md",
        include_str!("../bundled/code-review/SKILL.md"),
    ),
    (
        "data-analysis/SKILL.md",
        include_str!("../bundled/data-analysis/SKILL.md"),
    ),
    (
        "data-analysis/scripts/profile_data.py",
        include_str!("../bundled/data-analysis/scripts/profile_data.py"),
    ),
    ("debug/SKILL.md", include_str!("../bundled/debug/SKILL.md")),
    (
        "deep-research/SKILL.md",
        include_str!("../bundled/deep-research/SKILL.md"),
    ),
    ("docx/SKILL.md", include_str!("../bundled/docx/SKILL.md")),
    (
        "docx/scripts/docx_tool.py",
        include_str!("../bundled/docx/scripts/docx_tool.py"),
    ),
    (
        "explain-codebase/SKILL.md",
        include_str!("../bundled/explain-codebase/SKILL.md"),
    ),
    (
        "fix-tests/SKILL.md",
        include_str!("../bundled/fix-tests/SKILL.md"),
    ),
    (
        "frontend-design/SKILL.md",
        include_str!("../bundled/frontend-design/SKILL.md"),
    ),
    (
        "git-workflow/SKILL.md",
        include_str!("../bundled/git-workflow/SKILL.md"),
    ),
    (
        "init-project/SKILL.md",
        include_str!("../bundled/init-project/SKILL.md"),
    ),
    (
        "mcp-builder/SKILL.md",
        include_str!("../bundled/mcp-builder/SKILL.md"),
    ),
    ("pdf/SKILL.md", include_str!("../bundled/pdf/SKILL.md")),
    (
        "pdf/scripts/pdf_tool.py",
        include_str!("../bundled/pdf/scripts/pdf_tool.py"),
    ),
    ("plan/SKILL.md", include_str!("../bundled/plan/SKILL.md")),
    ("pptx/SKILL.md", include_str!("../bundled/pptx/SKILL.md")),
    (
        "pptx/scripts/pptx_tool.py",
        include_str!("../bundled/pptx/scripts/pptx_tool.py"),
    ),
    (
        "refactor/SKILL.md",
        include_str!("../bundled/refactor/SKILL.md"),
    ),
    (
        "security-review/SKILL.md",
        include_str!("../bundled/security-review/SKILL.md"),
    ),
    (
        "skill-creator/SKILL.md",
        include_str!("../bundled/skill-creator/SKILL.md"),
    ),
    (
        "write-tests/SKILL.md",
        include_str!("../bundled/write-tests/SKILL.md"),
    ),
    ("xlsx/SKILL.md", include_str!("../bundled/xlsx/SKILL.md")),
    (
        "xlsx/scripts/xlsx_tool.py",
        include_str!("../bundled/xlsx/scripts/xlsx_tool.py"),
    ),
];

/// Write bundled skills under `dest`, updating files whose content changed.
/// Returns how many files were written.
pub fn install_bundled(dest: &Path) -> std::io::Result<usize> {
    let mut written = 0;
    for (rel, content) in FILES {
        let path = dest.join(rel);
        if std::fs::read_to_string(&path)
            .map(|old| old == *content)
            .unwrap_or(false)
        {
            continue;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, content)?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SkillSet, SkillSource};

    #[test]
    fn bundled_skills_install_and_parse() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(install_bundled(d.path()).unwrap(), FILES.len());
        assert_eq!(
            install_bundled(d.path()).unwrap(),
            0,
            "second install should change nothing"
        );
        let set = SkillSet::discover(&[(d.path().to_path_buf(), SkillSource::Bundled)]);
        assert!(set.warnings.is_empty(), "{:?}", set.warnings);
        let names: Vec<&str> = set.all().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "browser",
                "code-review",
                "data-analysis",
                "debug",
                "deep-research",
                "docx",
                "explain-codebase",
                "fix-tests",
                "frontend-design",
                "git-workflow",
                "init-project",
                "mcp-builder",
                "pdf",
                "plan",
                "pptx",
                "refactor",
                "security-review",
                "skill-creator",
                "write-tests",
                "xlsx"
            ]
        );
        // The default listing budget still names every skill with a usable description.
        let listing = set.prompt_listing(3200);
        assert!(listing.len() <= 3200, "{}", listing.len());
        assert!(
            set.all()
                .all(|s| listing.contains(&format!("- {}: ", s.name))),
            "{listing}"
        );
        assert!(!listing.contains(" -…") && !listing.contains(",…"));
        // Roomy enough to keep about 100 characters of each description.
        assert!(listing.contains("- browser: Do a task on a website in a real browser - log in, click through pages, fill in"), "{listing}");
    }
}
