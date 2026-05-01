# Examples: Planning with Files in Action

> In this repo, planning files live under `Application/docs/plans/`, and any other long-form outputs should also go under `Application/` or `CloudProgram/`, not repo root.

## Example 1: Research Task

**User request:** "Research the benefits of morning exercise and write a summary"

### Loop 1: Create plan

```bash
Write Application/docs/plans/task_plan.md
```

```markdown
# Task Plan: Morning Exercise Benefits Research

## Goal
Create a research summary on the benefits of morning exercise.

## Phases
- [ ] Phase 1: Create this plan
- [ ] Phase 2: Search and gather sources
- [ ] Phase 3: Synthesize findings
- [ ] Phase 4: Deliver summary

## Key Questions
1. What are the physical health benefits?
2. What are the mental health benefits?
3. What scientific studies support this?

## Status
**Currently in Phase 1** - Creating the plan
```

### Loop 2: Research

```bash
Read Application/docs/plans/task_plan.md
WebSearch "morning exercise benefits"
Write Application/docs/plans/research-notes.md
Edit Application/docs/plans/task_plan.md
```

### Loop 3: Synthesize

```bash
Read Application/docs/plans/task_plan.md
Read Application/docs/plans/research-notes.md
Write Application/docs/session-notes/morning_exercise_summary.md
Edit Application/docs/plans/task_plan.md
```

### Loop 4: Deliver

```bash
Read Application/docs/plans/task_plan.md
Deliver Application/docs/session-notes/morning_exercise_summary.md
```

---

## Example 2: Bug Fix Task

**User request:** "Fix the login bug in the authentication module"

### Application/docs/plans/task_plan.md

```markdown
# Task Plan: Fix Login Bug

## Goal
Identify and fix the bug preventing successful login.

## Phases
- [x] Phase 1: Understand the bug report
- [x] Phase 2: Locate relevant code
- [ ] Phase 3: Identify root cause
- [ ] Phase 4: Implement fix
- [ ] Phase 5: Test and verify

## Key Questions
1. What error message appears?
2. Which file handles authentication?
3. What changed recently?

## Decisions Made
- Auth handler is in src/auth/login.ts
- Error occurs in validateToken()

## Errors Encountered
- TypeError: Cannot read property 'token' of undefined
  -> Root cause: user object was not awaited properly
```

---

## Example 3: Feature Development

**User request:** "Add a dark mode toggle to the settings page"

### The 3-file pattern

**Application/docs/plans/task_plan.md**

```markdown
# Task Plan: Dark Mode Toggle

## Goal
Add a functional dark mode toggle to settings.

## Phases
- [x] Phase 1: Research existing theme system
- [x] Phase 2: Design implementation approach
- [ ] Phase 3: Implement toggle component
- [ ] Phase 4: Add theme switching logic
- [ ] Phase 5: Test and polish
```

**Application/docs/plans/findings.md**

```markdown
# Findings and Decisions

## Existing Theme System
- Located in: src/styles/theme.ts
- Uses: CSS custom properties
- Current themes: light only

## Files to Modify
1. src/styles/theme.ts
2. src/components/SettingsPage.tsx
3. src/hooks/useTheme.ts
4. src/App.tsx
```

**Application/docs/session-notes/dark_mode_implementation.md**

```markdown
# Dark Mode Implementation

## Changes Made

### 1. Added dark theme colors
File: src/styles/theme.ts

### 2. Created useTheme hook
File: src/hooks/useTheme.ts
```

---

## Example 4: Error Recovery Pattern

When something fails, do not hide it.

### Before

```text
Action: Read config.json
Error: File not found
Action: Read config.json
Action: Read config.json
```

### After

```text
Action: Read config.json
Error: File not found

# Update Application/docs/plans/task_plan.md:
## Errors Encountered
- config.json not found -> Will create default config

Action: Write config.json (default config)
Action: Read config.json
Success
```

---

## The Read-Before-Decide Pattern

Always re-read the plan before major decisions.

```text
[Many tool calls have happened]
[Context is getting long]
[The original goal might be fading]

-> Read Application/docs/plans/task_plan.md
-> Make the decision with the goal back in context
```
