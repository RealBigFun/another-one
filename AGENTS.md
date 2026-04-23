# AGENTS

This is a greenfield app, and has no users.
This app is built for mac and linux - any changes must keep this in mind.

## UI Rules

- This applies to icon-only controls and text-based actions alike unless the element is purely decorative or intentionally non-interactive.
- Any user-facing errors or notifications should go through the app toast function unless explicitly specified otherwise.

## Tracking work

We use GitHub Issues to capture work. Do not let drive-by findings expand the current branch's scope — file them and move on.

**When to file an issue instead of fixing in place**
- You notice a bug/smell/idea that is not part of the current task.
- The user says "idea:" / "later:" / "while you're here" / anything not on the current branch's stated goal.
- Work is blocked and needs external input.

**How to file — always confirm first**
- Never open an issue silently. Propose title + label + one-line body to the user and wait for a yes. Applies to both user idea-dumps and agent-spotted drive-by findings.
- Search first: `gh issue list --search "<keywords>"` — no duplicates.
- After approval: `gh issue create --title "<imperative, short>" --label <label> --body "..."`.
- Body: one line on what you were doing when you noticed it, plus file/line if relevant.

**Labels**
- `bug` — something is broken
- `enhancement` — new capability or improvement to existing behavior
- `idea` — raw brain-dump, not yet triaged
- `question` — needs clarification before work can start

**Finding the next piece of work**
- `gh issue list --state open` to browse, or filter by label (`--label enhancement`, `--label bug`, etc.).
- If picking one up, reference it in the PR (`Closes #123`).

**Never**
- Open an issue without first getting the user's OK on title + label.
- Close issues you did not fix — let the user triage `idea` / `question`.
- Open an issue and then immediately fix it on the current branch. If it's small enough to fix right now, it was part of the current task; if it wasn't, it doesn't belong on this branch.
