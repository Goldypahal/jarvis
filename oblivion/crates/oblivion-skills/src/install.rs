//! `oblivion --install-skill <github url>` (SRS FR-34): fetch one folder of a
//! GitHub repo with a sparse git checkout and copy every skill in it.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{find_skill_dirs, parse_skill_md};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubSource {
    pub owner: String,
    pub repo: String,
    pub git_ref: Option<String>,
    pub path: String,
}

impl GithubSource {
    /// Accepts `https://github.com/o/r`, `https://github.com/o/r/tree/<ref>/<path>`,
    /// or the short form `o/r/<path>`.
    pub fn parse(input: &str) -> Result<Self, String> {
        let s = input.trim().trim_end_matches('/');
        let s = s
            .strip_prefix("https://github.com/")
            .or_else(|| s.strip_prefix("http://github.com/"))
            .or_else(|| s.strip_prefix("github.com/"))
            .unwrap_or(s);
        let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 {
            return Err(format!(
                "'{input}' is not a GitHub repo or folder (expected owner/repo[/path])"
            ));
        }
        let (owner, repo) = (
            parts[0].to_string(),
            parts[1].trim_end_matches(".git").to_string(),
        );
        let (git_ref, path) = if parts.len() >= 4 && (parts[2] == "tree" || parts[2] == "blob") {
            (Some(parts[3].to_string()), parts[4..].join("/"))
        } else {
            (None, parts[2..].join("/"))
        };
        let path = path
            .trim_end_matches("/SKILL.md")
            .trim_end_matches("SKILL.md")
            .trim_end_matches('/')
            .to_string();
        Ok(Self {
            owner,
            repo,
            git_ref,
            path,
        })
    }
}

/// One installed skill, for the report printed to the user.
#[derive(Debug, Clone)]
pub struct Installed {
    pub name: String,
    pub dir: PathBuf,
    pub license: String,
}

fn git(args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
    let mut c = Command::new("git");
    c.args(args);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    let out = c
        .output()
        .map_err(|e| format!("could not run git (is it installed?): {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let name = e.file_name();
        if name == ".git" {
            continue;
        }
        let (src, dst) = (e.path(), to.join(&name));
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// What a skill says about its licence: frontmatter first, then a LICENSE file.
fn license_of(dir: &Path, front: Option<String>) -> String {
    if let Some(l) = front.filter(|l| !l.trim().is_empty()) {
        return l;
    }
    for name in ["LICENSE", "LICENSE.txt", "LICENSE.md", "license.txt"] {
        if let Ok(t) = std::fs::read_to_string(dir.join(name)) {
            if let Some(first) = t.lines().find(|l| !l.trim().is_empty()) {
                return first.trim().chars().take(80).collect();
            }
        }
    }
    "not stated — check the repo before sharing it".into()
}

/// Install every skill under `src` into `dest_root/<skill name>`, replacing older copies.
pub fn install_from_github(src: &GithubSource, dest_root: &Path) -> Result<Vec<Installed>, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("oblivion-skill-{}-{stamp}", std::process::id()));
    let url = format!("https://github.com/{}/{}.git", src.owner, src.repo);
    let tmp_s = tmp.to_string_lossy().into_owned();

    let result = (|| {
        let mut args = vec!["clone", "--depth", "1", "--filter=blob:none", "--sparse"];
        if let Some(r) = &src.git_ref {
            args.extend(["--branch", r.as_str()]);
        }
        args.extend([url.as_str(), tmp_s.as_str()]);
        git(&args, None)?;
        if src.path.is_empty() {
            git(&["sparse-checkout", "disable"], Some(&tmp))?;
        } else {
            git(
                &[
                    "sparse-checkout",
                    "set",
                    "--no-cone",
                    &format!("/{}/", src.path),
                ],
                Some(&tmp),
            )?;
        }
        let base = tmp.join(&src.path);
        if !base.is_dir() {
            return Err(format!(
                "'{}' was not found in {}/{}",
                src.path, src.owner, src.repo
            ));
        }
        let mut dirs = Vec::new();
        find_skill_dirs(&base, 3, &mut dirs);
        if dirs.is_empty() {
            return Err(format!(
                "no SKILL.md found under {}",
                if src.path.is_empty() {
                    "the repo"
                } else {
                    &src.path
                }
            ));
        }
        let mut installed = Vec::new();
        for dir in dirs {
            let dir_name = dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("skill")
                .to_string();
            let text = std::fs::read_to_string(dir.join("SKILL.md")).map_err(|e| e.to_string())?;
            let (name, _desc, front_license, _body) =
                parse_skill_md(&text, &dir_name).map_err(|e| format!("{dir_name}: {e}"))?;
            let dest = dest_root.join(&name);
            if dest.exists() {
                std::fs::remove_dir_all(&dest)
                    .map_err(|e| format!("cannot replace {}: {e}", dest.display()))?;
            }
            copy_dir(&dir, &dest).map_err(|e| format!("copy failed: {e}"))?;
            let license = license_of(&dest, front_license);
            installed.push(Installed {
                name,
                dir: dest,
                license,
            });
        }
        Ok(installed)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_forms() {
        let g = GithubSource::parse(
            "https://github.com/anthropics/skills/tree/main/skills/mcp-builder",
        )
        .unwrap();
        assert_eq!(
            g,
            GithubSource {
                owner: "anthropics".into(),
                repo: "skills".into(),
                git_ref: Some("main".into()),
                path: "skills/mcp-builder".into()
            }
        );
        let g = GithubSource::parse("openai/skills/skills/.curated/gh-fix-ci/").unwrap();
        assert_eq!(
            (g.git_ref, g.path.as_str()),
            (None, "skills/.curated/gh-fix-ci")
        );
        let g = GithubSource::parse("https://github.com/o/r.git").unwrap();
        assert_eq!((g.repo.as_str(), g.path.as_str()), ("r", ""));
        let g = GithubSource::parse("https://github.com/o/r/blob/main/x/SKILL.md").unwrap();
        assert_eq!(g.path, "x");
        assert!(GithubSource::parse("nonsense").is_err());
    }

    #[test]
    fn license_lookup() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(license_of(d.path(), Some("MIT".into())), "MIT");
        std::fs::write(
            d.path().join("LICENSE.txt"),
            "\n   Apache License\n Version 2.0",
        )
        .unwrap();
        assert_eq!(license_of(d.path(), None), "Apache License");
    }
}
