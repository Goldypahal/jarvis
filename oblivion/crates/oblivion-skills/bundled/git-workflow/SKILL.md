---
name: git-workflow
description: Commit, branch, write commit messages and PR descriptions, and resolve merge conflicts with git. Use when the user asks to commit, make a branch, write a PR, or fix a merge conflict.
license: MIT
---
# Git workflow

Commit
1. `git status` and `git diff` to see what changed. Do not commit files that look like secrets (.env, keys) or build output.
2. Stage only what belongs to this change: `git add <paths>`.
3. Message: short summary line (≤ 72 chars, imperative: "Fix login timeout"), blank line, then why and what in a few lines.
4. `git commit -m "<summary>" -m "<body>"`. Show the resulting `git log --oneline -1`.

Branch / PR
- New work: `git switch -c <type>/<short-name>` (feat/, fix/, chore/).
- PR text: what changed, why, how it was tested, anything reviewers should look at.

Merge conflicts
1. `git status` lists conflicted files. For each, read both sides between `<<<<<<<`, `=======`, `>>>>>>>`.
2. Keep the intent of both sides; never just pick one blindly. Remove all markers.
3. Build/test, then `git add` the files and continue (`git merge --continue` or `git rebase --continue`).

Never run `git push --force`, `git reset --hard` or delete branches unless the user explicitly asked for exactly that. Pushing is the user's decision: ask first.
