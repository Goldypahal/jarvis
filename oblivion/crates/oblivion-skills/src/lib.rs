//! Agent Skills (SRS FR-30..FR-36).
//!
//! A skill is a folder with a `SKILL.md` file: YAML frontmatter (`name`,
//! `description`, optional `license`, `allowed-tools`, `metadata`) and
//! Markdown instructions, plus any scripts or reference files it needs.
//! This is the open format used by Claude Code, Codex and many other agents,
//! so their skills work here unchanged.
//!
//! Progressive disclosure keeps requests small (important on free tiers):
//! only each skill's name and description go in the system prompt; the
//! `skill` tool loads the full instructions when the model decides to use it.

mod bundled;
mod install;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oblivion_providers::ToolSpec;
use oblivion_tools::{Risk, ToolFuture, ToolProvider};
use serde::Deserialize;
use serde_json::{json, Value};

pub use bundled::install_bundled;
pub use install::{install_from_github, GithubSource};

/// Where a skill was found. Later sources override earlier ones with the same name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillSource {
    /// Shared folders also used by Claude Code, Codex and other agents.
    External,
    Bundled,
    User,
    Project,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub dir: PathBuf,
    pub source: SkillSource,
    pub license: Option<String>,
    /// Instructions (the Markdown after the frontmatter).
    pub body: String,
}

#[derive(Debug, Default, Deserialize)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    license: Option<String>,
}

/// Split `SKILL.md` into frontmatter fields and body.
pub fn parse_skill_md(
    text: &str,
    dir_name: &str,
) -> Result<(String, String, Option<String>, String), String> {
    let text = text.trim_start_matches('\u{feff}');
    let (front, body) = match text.strip_prefix("---") {
        Some(rest) => {
            // The closing fence is the first line that is exactly "---".
            let mut offset = 0;
            let mut found = None;
            for line in rest.split_inclusive('\n') {
                if offset > 0 && line.trim_end() == "---" {
                    found = Some((offset, offset + line.len()));
                    break;
                }
                offset += line.len();
            }
            let (end, after) = found.ok_or("frontmatter is not closed with ---")?;
            (&rest[..end], &rest[after..])
        }
        None => ("", text),
    };
    let fm: Frontmatter = if front.trim().is_empty() {
        Frontmatter::default()
    } else {
        serde_yaml::from_str(front).map_err(|e| format!("invalid frontmatter: {e}"))?
    };
    let name = fm
        .name
        .unwrap_or_else(|| dir_name.to_string())
        .trim()
        .to_string();
    let description = fm
        .description
        .map(|d| d.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|d| !d.is_empty())
        .ok_or("description is required in the frontmatter")?;
    Ok((name, description, fm.license, body.trim().to_string()))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Find `SKILL.md` files up to `depth` folders below `root`.
/// Folders never searched for skills: hidden folders (Codex keeps its own
/// system skills in `.system`) and `synced` (the Claude app's cache of skills
/// written for Claude's own tools, which Oblivion does not have).
fn skipped_dir(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.') || n == "synced" || n == "node_modules")
}

fn find_skill_dirs(root: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    find_skill_dirs_inner(root, depth, true, out)
}

pub(crate) fn find_skill_dirs_inner(
    root: &Path,
    depth: usize,
    skip_hidden: bool,
    out: &mut Vec<PathBuf>,
) {
    if root.join("SKILL.md").is_file() {
        out.push(root.to_path_buf());
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !(skip_hidden && skipped_dir(p)))
        .collect();
    dirs.sort();
    for d in dirs {
        find_skill_dirs_inner(&d, depth - 1, skip_hidden, out);
    }
}

#[derive(Debug, Default, Clone)]
pub struct SkillSet {
    skills: BTreeMap<String, Skill>,
    /// Problems found while loading, shown by `oblivion --skills`.
    pub warnings: Vec<String>,
}

impl SkillSet {
    /// Load skills from `(folder, source)` pairs, lowest priority first.
    pub fn discover(roots: &[(PathBuf, SkillSource)]) -> Self {
        let mut set = SkillSet::default();
        for (root, source) in roots {
            let mut dirs = Vec::new();
            find_skill_dirs(root, 3, &mut dirs);
            for dir in dirs {
                let dir_name = dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("skill")
                    .to_string();
                let text = match std::fs::read_to_string(dir.join("SKILL.md")) {
                    Ok(t) => t,
                    Err(e) => {
                        set.warnings.push(format!("{}: {e}", dir.display()));
                        continue;
                    }
                };
                match parse_skill_md(&text, &dir_name) {
                    Ok((name, description, license, body)) => {
                        if !valid_name(&name) {
                            set.warnings.push(format!(
                                "{}: skill name '{name}' must be lowercase letters, digits and dashes",
                                dir.display()
                            ));
                            continue;
                        }
                        set.skills.insert(
                            name.clone(),
                            Skill {
                                name,
                                description,
                                dir,
                                source: *source,
                                license,
                                body,
                            },
                        );
                    }
                    Err(e) => set
                        .warnings
                        .push(format!("{}: {e}", dir.join("SKILL.md").display())),
                }
            }
        }
        set
    }

    /// The standard places skills live, lowest priority first: bundled, the
    /// user's folders (Oblivion, cross-agent, Claude, Codex), then the project's.
    pub fn standard_roots(
        oblivion_home: &Path,
        user_home: Option<&Path>,
        project: &Path,
    ) -> Vec<(PathBuf, SkillSource)> {
        // Lowest priority first. Skills in folders shared with other agents come
        // first so Oblivion's own version of a same-named skill wins: those copies
        // are often written for another agent's tools.
        let mut roots = Vec::new();
        if let Some(h) = user_home {
            for sub in [".agents/skills", ".claude/skills", ".codex/skills"] {
                roots.push((h.join(sub), SkillSource::External));
            }
        }
        roots.push((oblivion_home.join("bundled-skills"), SkillSource::Bundled));
        roots.push((oblivion_home.join("skills"), SkillSource::User));
        for sub in [".agents/skills", ".claude/skills", ".oblivion/skills"] {
            roots.push((project.join(sub), SkillSource::Project));
        }
        roots
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn all(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values()
    }

    /// The skills section of the system prompt, kept under `max_chars`.
    pub fn prompt_listing(&self, max_chars: usize) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let header = "Skills\n\
Skills are folders of expert instructions. When a request matches a skill's description, \
call the `skill` tool with that name BEFORE doing the work, then follow its instructions \
exactly. Scripts it mentions live in its base folder; run them with the shell tool.\n\
Available skills:\n";
        // Shorten descriptions step by step until the list fits.
        for limit in [400usize, 250, 160, 100, 60] {
            let mut out = String::from(header);
            for s in self.skills.values() {
                let cut = s.description.chars().count() > limit;
                let mut d: String = s.description.chars().take(limit).collect();
                if cut {
                    // End on a whole word so the meaning survives the cut.
                    if let Some(i) = d.rfind(' ') {
                        d.truncate(i);
                    }
                    d = d.trim_end_matches([',', ';', ':', '-', ' ']).to_string();
                }
                let ell = if cut { "…" } else { "" };
                out.push_str(&format!("- {}: {d}{ell}\n", s.name));
            }
            if out.len() <= max_chars {
                return out;
            }
        }
        let mut out = String::from(header);
        for s in self.skills.values() {
            let line = format!("- {}\n", s.name);
            if out.len() + line.len() > max_chars {
                break;
            }
            out.push_str(&line);
        }
        out
    }

    /// What the `skill` tool returns: the instructions plus where its files are.
    pub fn load_text(&self, name: &str) -> Result<String, String> {
        let s = self.get(name).ok_or_else(|| {
            format!(
                "no skill named '{name}'. Available: {}",
                self.skills.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })?;
        let mut files = Vec::new();
        list_files(&s.dir, &s.dir, &mut files);
        files.retain(|f| f != "SKILL.md");
        files.sort();
        let listing = if files.is_empty() {
            String::new()
        } else {
            let shown: Vec<&String> = files.iter().take(60).collect();
            let more = files.len().saturating_sub(60);
            format!(
                "\nFiles in this skill (read with read_file, run scripts with the shell tool):\n{}{}\n",
                shown.iter().map(|f| format!("  {f}\n")).collect::<String>(),
                if more > 0 { format!("  … {more} more\n") } else { String::new() }
            )
        };
        Ok(format!(
            "Skill `{}` loaded.\nBase folder: {}\nRelative paths in these instructions are relative to the base folder.\n{listing}\n<skill_instructions>\n{}\n</skill_instructions>",
            s.name,
            s.dir.display(),
            s.body
        ))
    }
}

fn list_files(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "__pycache__" || name == "node_modules" {
            continue;
        }
        if p.is_dir() {
            list_files(base, &p, out);
        } else if let Ok(rel) = p.strip_prefix(base) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

impl ToolProvider for SkillSet {
    fn specs(&self) -> Vec<ToolSpec> {
        if self.skills.is_empty() {
            return vec![];
        }
        vec![ToolSpec {
            name: "skill".into(),
            description: "Load a skill's full instructions by name (see the Skills list in the system prompt). \
                Call it before starting work the skill covers."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {"name": {"type": "string", "description": "Skill name"}},
                "required": ["name"]
            }),
        }]
    }

    fn handles(&self, name: &str) -> bool {
        name == "skill"
    }

    fn risk(&self, _name: &str, _args: &Value) -> Risk {
        Risk::Safe
    }

    fn subject(&self, _name: &str, args: &Value) -> String {
        args.get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    }

    fn execute<'a>(&'a self, _name: &'a str, args: &'a Value) -> ToolFuture<'a> {
        let result = match args.get("name").and_then(Value::as_str) {
            Some(n) => self.load_text(n.trim()),
            None => Err("missing required argument 'name'".into()),
        };
        Box::pin(async move { result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, dir: &str, text: &str) {
        let d = root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("SKILL.md"), text).unwrap();
    }

    #[test]
    fn parses_frontmatter_variants() {
        let (n, d, l, b) = parse_skill_md(
            "---\nname: pdf\ndescription: >\n  Work with PDFs:\n  merge, split.\nlicense: Apache-2.0\nmetadata:\n  version: 1\n---\n# PDF\nSteps\n",
            "x",
        )
        .unwrap();
        assert_eq!(
            (n.as_str(), d.as_str(), l.as_deref(), b.as_str()),
            (
                "pdf",
                "Work with PDFs: merge, split.",
                Some("Apache-2.0"),
                "# PDF\nSteps"
            )
        );
        let (n, ..) =
            parse_skill_md("---\ndescription: \"quoted: colon\"\n---\nbody", "dir-name").unwrap();
        assert_eq!(n, "dir-name");
        assert!(parse_skill_md("---\nname: x\n---\nbody", "x")
            .unwrap_err()
            .contains("description"));
        assert!(parse_skill_md("---\nname: x\ndescription: y\n", "x").is_err());
    }

    #[test]
    fn discovery_precedence_and_warnings() {
        let d = tempfile::tempdir().unwrap();
        let bundled = d.path().join("b");
        let user = d.path().join("u");
        let project = d.path().join("p");
        write_skill(
            &bundled,
            "review",
            "---\nname: review\ndescription: bundled review\n---\nB",
        );
        write_skill(
            &user,
            "nested/team/review",
            "---\nname: review\ndescription: user review\n---\nU",
        );
        write_skill(
            &project,
            "Bad Name",
            "---\nname: Bad Name\ndescription: x\n---\n",
        );
        write_skill(
            &project,
            "notes",
            "---\nname: notes\ndescription: take notes\n---\nN",
        );
        let set = SkillSet::discover(&[
            (bundled, SkillSource::Bundled),
            (user, SkillSource::User),
            (project, SkillSource::Project),
            (d.path().join("missing"), SkillSource::User),
        ]);
        assert_eq!(set.len(), 2);
        assert_eq!(set.get("review").unwrap().description, "user review");
        assert_eq!(set.get("review").unwrap().source, SkillSource::User);
        assert_eq!(set.warnings.len(), 1);
    }

    #[test]
    fn standard_roots_prefer_oblivion_over_other_agents_and_skip_caches() {
        let d = tempfile::tempdir().unwrap();
        let (home, user, proj) = (d.path().join("obl"), d.path().join("u"), d.path().join("p"));
        write_skill(
            &user.join(".claude/skills/synced/abc"),
            "deep-research",
            "---\nname: deep-research\ndescription: claude app copy\n---\nC",
        );
        write_skill(
            &user.join(".codex/skills/.system"),
            "skill-creator",
            "---\nname: skill-creator\ndescription: codex system copy\n---\nX",
        );
        write_skill(
            &user.join(".claude/skills"),
            "plan",
            "---\nname: plan\ndescription: claude user plan\n---\nP",
        );
        write_skill(
            &user.join(".claude/skills"),
            "mine",
            "---\nname: mine\ndescription: my own claude skill\n---\nM",
        );
        write_skill(
            &home.join("bundled-skills"),
            "plan",
            "---\nname: plan\ndescription: oblivion plan\n---\nO",
        );
        let set = SkillSet::discover(&SkillSet::standard_roots(&home, Some(&user), &proj));
        assert_eq!(set.get("plan").unwrap().description, "oblivion plan");
        assert_eq!(set.get("mine").unwrap().source, SkillSource::External);
        assert!(
            set.get("deep-research").is_none(),
            "Claude app cache must be skipped"
        );
        assert!(
            set.get("skill-creator").is_none(),
            "hidden folders must be skipped"
        );
    }

    #[test]
    fn listing_fits_budget_and_tool_loads() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..20 {
            write_skill(
                d.path(),
                &format!("s{i}"),
                &format!(
                    "---\nname: s{i}\ndescription: {}\n---\nBody {i}",
                    "word ".repeat(80)
                ),
            );
        }
        std::fs::create_dir_all(d.path().join("s3/scripts")).unwrap();
        std::fs::write(d.path().join("s3/scripts/run.py"), "print(1)").unwrap();
        let set = SkillSet::discover(&[(d.path().to_path_buf(), SkillSource::User)]);
        let listing = set.prompt_listing(3000);
        assert!(listing.len() <= 3000, "{}", listing.len());
        assert!(listing.contains("- s19"));
        let text = set.load_text("s3").unwrap();
        assert!(
            text.contains("scripts/run.py")
                && text.contains("Body 3")
                && text.contains("Base folder:")
        );
        assert!(set.load_text("nope").unwrap_err().contains("Available"));
    }
}
