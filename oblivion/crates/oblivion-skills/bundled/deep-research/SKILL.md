---
name: deep-research
description: Research a question across many web sources and write a cited report. Use when the user asks to research, compare options, investigate a topic, find the latest on something, or wants a report with sources.
license: MIT
---
# Deep research

1. Restate the question in one line. If the scope is unclear (time period, region, depth), ask ONE question and stop.
2. Split it into 3–5 sub-questions that together answer it. Put them in the `todo` tool.
3. In ONE response, call the `task` tool once per sub-question so they run in parallel. Each prompt must stand alone, e.g.:
   "Research: <sub-question>. Use web_search with 2–3 different queries, then web_fetch the 2–4 most relevant
   pages (official sources first). Report: key facts with numbers and dates, each followed by its source URL;
   note disagreements between sources; say what you could not find. Under 400 words."
4. Read the reports. If something important is missing or sources disagree, run one more round of `task` calls
   on just those gaps (at most two rounds in total).
5. Write the report to a Markdown file (default `research-<topic>.md` in the workspace) with:
   - **Answer** — 3–5 sentences that directly answer the question.
   - **Findings** — sections per sub-question; every factual claim followed by its source as `[n]`.
   - **Disagreements and gaps** — where sources conflict or evidence is thin.
   - **Sources** — numbered list `[n] Title — URL`.
6. Only cite URLs that a sub-agent actually fetched or found. Never invent sources, numbers or quotes.
   Only fetch URLs that came from a search result or a fetched page; never guess or construct URLs.

Budget (free models have small daily limits; one research run must not use them all):
- At most two rounds of `task` calls. Each sub-agent has about 8 searches/fetches.
- If `task` is unavailable, research yourself: at most 10 searches/fetches in total, several per response,
  then write the report. Do not keep fetching "one more page".
- If you are told the web budget or steps are running out, write the report immediately with what you have
  and list the gaps.
7. If the user wants Word or PDF, convert the Markdown with the docx skill.
8. Reply with the answer in 3–5 sentences and the file path.
