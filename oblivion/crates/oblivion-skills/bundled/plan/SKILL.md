---
name: plan
description: Make a step-by-step implementation plan before changing code. Use for features or changes touching more than two files, when the user asks for a plan, or when requirements are unclear.
license: MIT
---
# Plan before building

1. Restate the goal in one sentence. If something essential is ambiguous, ask ONE question and stop.
2. Explore: find the relevant files with `grep`/`glob` and read them. Note how similar things are already done.
3. Write the plan as a numbered list. Each step names the file(s) to change and what changes. Include:
   - tests to add or update,
   - commands to verify (build, tests),
   - risks or open questions.
4. Put the steps in the `todo` tool as well.
5. Show the plan and ask the user to approve it. Do not edit files until they say yes.
6. After approval, work through the todo list in order, marking each step done, and run the verification commands at the end.
