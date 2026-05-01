#!/bin/bash
# Initialize planning files for a new session
# Usage: ./init-session.sh [project-name] [plan-dir]

set -e

PROJECT_NAME="${1:-project}"
PLAN_DIR="${2:-Application/docs/plans}"
DATE=$(date +%Y-%m-%d)
TASK_PLAN_FILE="$PLAN_DIR/task_plan.md"
FINDINGS_FILE="$PLAN_DIR/findings.md"
PROGRESS_FILE="$PLAN_DIR/progress.md"

echo "Initializing planning files for: $PROJECT_NAME"
mkdir -p "$PLAN_DIR"

if [ ! -f "$TASK_PLAN_FILE" ]; then
    cat > "$TASK_PLAN_FILE" << 'EOF'
# Task Plan: [Brief Description]

## Goal
[One sentence describing the end state]

## Current Phase
Phase 1

## Phases

### Phase 1: Requirements and Discovery
- [ ] Understand user intent
- [ ] Identify constraints
- [ ] Document in findings.md
- **Status:** in_progress

### Phase 2: Planning and Structure
- [ ] Define approach
- [ ] Create project structure
- **Status:** pending

### Phase 3: Implementation
- [ ] Execute the plan
- [ ] Write to files before executing
- **Status:** pending

### Phase 4: Testing and Verification
- [ ] Verify requirements are met
- [ ] Document test results
- **Status:** pending

### Phase 5: Delivery
- [ ] Review outputs
- [ ] Deliver to user
- **Status:** pending

## Decisions Made
| Decision | Rationale |
|----------|-----------|

## Errors Encountered
| Error | Resolution |
|-------|------------|
EOF
    echo "Created $TASK_PLAN_FILE"
else
    echo "$TASK_PLAN_FILE already exists, skipping"
fi

if [ ! -f "$FINDINGS_FILE" ]; then
    cat > "$FINDINGS_FILE" << 'EOF'
# Findings and Decisions

## Requirements
-

## Research Findings
-

## Technical Decisions
| Decision | Rationale |
|----------|-----------|

## Issues Encountered
| Issue | Resolution |
|-------|------------|

## Resources
-
EOF
    echo "Created $FINDINGS_FILE"
else
    echo "$FINDINGS_FILE already exists, skipping"
fi

if [ ! -f "$PROGRESS_FILE" ]; then
    cat > "$PROGRESS_FILE" << EOF
# Progress Log

## Session: $DATE

### Current Status
- **Phase:** 1 - Requirements and Discovery
- **Started:** $DATE

### Actions Taken
-

### Test Results
| Test | Expected | Actual | Status |
|------|----------|--------|--------|

### Errors
| Error | Resolution |
|-------|------------|
EOF
    echo "Created $PROGRESS_FILE"
else
    echo "$PROGRESS_FILE already exists, skipping"
fi

echo ""
echo "Planning files initialized in $PLAN_DIR"
echo "Files: $TASK_PLAN_FILE, $FINDINGS_FILE, $PROGRESS_FILE"
