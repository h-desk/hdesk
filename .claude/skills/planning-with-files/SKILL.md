---
name: planning-with-files
version: "2.0.1"
description: Implements Manus-style file-based planning for complex tasks. Creates `Application/docs/plans/task_plan.md`, `Application/docs/plans/findings.md`, and `Application/docs/plans/progress.md`. Use when starting complex multi-step tasks, research projects, or any task requiring more than 5 tool calls.
allowed-tools: Read, Write, Edit, Bash, Glob, Grep, WebFetch, WebSearch
hooks:
  PreToolUse:
    - matcher: "Write|Edit|Bash"
      hooks:
        - type: command
          command: "cat Application/docs/plans/task_plan.md 2>/dev/null | head -30 || true"
  Stop:
    - hooks:
        - type: command
          command: "${CLAUDE_PLUGIN_ROOT}/scripts/check-complete.sh"
---

# Planning with Files

Work like Manus: use persistent markdown files as working memory on disk.

## Important: Where Files Go

When using this skill:

- Templates stay in the skill directory at `${CLAUDE_PLUGIN_ROOT}/templates/`.
- Planning files live under `Application/docs/plans/` in the current repo.
- Other long-form notes, reports, and similar artifacts should also go under `Application/` or `CloudProgram/`, not repo root.

| Location | What Goes There |
|----------|-----------------|
| Skill directory (`${CLAUDE_PLUGIN_ROOT}/`) | Templates, scripts, reference docs |
| `Application/docs/plans/` | `task_plan.md`, `findings.md`, `progress.md` |

This keeps repo root clean while keeping planning state inside the application project tree.

> Bare names like `task_plan.md` below always mean the file inside `Application/docs/plans/`.

## Quick Start

Before any complex task:

1. Create `Application/docs/plans/task_plan.md` from [templates/task_plan.md](templates/task_plan.md).
2. Create `Application/docs/plans/findings.md` from [templates/findings.md](templates/findings.md).
3. Create `Application/docs/plans/progress.md` from [templates/progress.md](templates/progress.md).
4. Re-read the plan before major decisions.
5. Update the planning files after each phase, discovery, or error.

## The Core Pattern

```text
Context Window = RAM (volatile, limited)
Filesystem = Disk (persistent, unlimited)

Anything important gets written to disk.
```

## File Purposes

| File | Purpose | When to Update |
|------|---------|----------------|
| `task_plan.md` | Phases, progress, decisions | After each phase |
| `findings.md` | Research, discoveries | After any meaningful finding |
| `progress.md` | Session log, verification notes | Throughout the task |

## Critical Rules

### 1. Create the plan first

Never start a complex task without `Application/docs/plans/task_plan.md`.

### 2. Use the 2-action rule

After every 2 view, browser, or search operations, write down the useful result in a planning file. This prevents visual or web findings from disappearing out of context.

### 3. Read before you decide

Before a major decision, re-read `task_plan.md` so the goal and current phase are back in the attention window.

### 4. Update after you act

After each phase:

- Mark the phase status.
- Log errors and blockers.
- Record files created or modified.

### 5. Log all errors

Every meaningful error goes in the planning files.

```markdown
## Errors Encountered
| Error | Attempt | Resolution |
|-------|---------|------------|
| FileNotFoundError | 1 | Created default config |
| API timeout | 2 | Added retry logic |
```

### 6. Never repeat the same failure

```text
if action_failed:
    next_action != same_action
```

Track what you tried, then change the approach.

## The 3-Strike Error Protocol

```text
ATTEMPT 1: Diagnose and fix
  -> Read the error carefully
  -> Identify the root cause
  -> Apply a targeted fix

ATTEMPT 2: Try a different method
  -> Use a different tool, entrypoint, or sequence
  -> Do not repeat the exact same failing action

ATTEMPT 3: Rethink the plan
  -> Challenge assumptions
  -> Search for missing context
  -> Update the plan if needed

AFTER 3 FAILURES: Escalate to the user
  -> Explain what you tried
  -> Share the actual error
  -> State what information is missing
```

## Read vs. Write Decision Matrix

| Situation | Action | Reason |
|-----------|--------|--------|
| Just wrote a file | Do not read it immediately | The content is still in context |
| Viewed an image or PDF | Write findings now | Visual details are easy to lose |
| Browser returned useful data | Store it in a file | Web data will scroll out of context |
| Starting a new phase | Read the plan and findings | Refresh goals and state |
| Error occurred | Read the relevant planning file | Avoid repeating a bad path |
| Resuming after a gap | Read all planning files | Rebuild context quickly |

## The 5-Question Reboot Test

| Question | Answer Source |
|----------|---------------|
| Where am I? | Current phase in `task_plan.md` |
| Where am I going? | Remaining phases in `task_plan.md` |
| What is the goal? | Goal statement in `task_plan.md` |
| What have I learned? | `findings.md` |
| What have I done? | `progress.md` |

## When to Use This Pattern

Use it for:

- Multi-step tasks
- Research tasks
- Tasks spanning many tool calls
- Work that needs persistent reasoning state

Skip it for:

- Simple questions
- Single-file edits
- Quick lookups

## Templates

Start from:

- [templates/task_plan.md](templates/task_plan.md)
- [templates/findings.md](templates/findings.md)
- [templates/progress.md](templates/progress.md)

## Scripts

- `scripts/init-session.sh [project-name] [plan-dir]`
  - Default plan directory: `Application/docs/plans`
- `scripts/check-complete.sh [plan-file]`
  - Default plan file: `Application/docs/plans/task_plan.md`

## Advanced Topics

- Manus principles: [reference.md](reference.md)
- Worked examples: [examples.md](examples.md)

## Anti-Patterns

| Don't | Do Instead |
|-------|------------|
| Use TodoWrite as the only memory | Create `Application/docs/plans/task_plan.md` |
| State goals once and forget them | Re-read the plan before major decisions |
| Hide errors and silently retry | Log the error and change approach |
| Keep everything only in context | Store large or important findings in files |
| Start execution immediately | Create the plan first |
| Repeat failed actions | Track attempts and mutate the approach |
| Create project files at repo root | Create them under `Application/` or `CloudProgram/` |
