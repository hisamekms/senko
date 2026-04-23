# Dependency Graph

Visualize task dependencies as a text-based graph for terminal display.

## Procedure

1. Fetch all tasks (walk every page — `task list` is cursor-paginated):

```bash
CURSOR=""
while :; do
  if [ -z "$CURSOR" ]; then
    PAGE=$(senko task list --limit 50)
  else
    PAGE=$(senko task list --limit 50 --after "$CURSOR")
  fi
  echo "$PAGE" | jq '.items[]'
  CURSOR=$(echo "$PAGE" | jq -r '.next_cursor // empty')
  [ -z "$CURSOR" ] && break
done
```

2. From the streamed task items, build a dependency graph. Each task's `dependencies` array lists the IDs it depends on.

3. Render the graph as a text-based diagram using these conventions:

- **Status indicators**: `[✓]` completed, `[▶]` in_progress, `[ ]` todo, `[-]` draft, `[✗]` canceled
- **Priority badge**: `(P0)` through `(P3)`
- **Arrows**: Use `→` to show dependency flow (from dependency to dependent task)
- **Layers**: Group tasks by depth in the dependency tree (root tasks first, then their dependents)
- Use box-drawing characters (`─`, `│`, `├`, `└`, `┬`) for connectors

## Example output

Given tasks: #1 (completed), #2 depends on #1 (in_progress), #3 depends on #1 (todo), #4 depends on #2 and #3 (todo), #5 (no deps, draft):

```
Dependency Graph
================

[✓] #1 Setup database (P0)
 ├──→ [▶] #2 Implement API (P1)
 │     └──→ [ ] #4 Deploy to staging (P2)
 └──→ [ ] #3 Write tests (P1)
       └──→ [ ] #4 Deploy to staging (P2)

[-] #5 Update docs (P3)

Legend: [✓] completed  [▶] in_progress  [ ] todo  [-] draft  [✗] canceled
```

For tasks with multiple dependencies (like #4 above), they appear under each parent. This makes all dependency paths visible.

## When a task has many dependencies

If the graph is large, also provide a flat summary table after the graph:

```
Task Dependencies Summary
==========================
#1  Setup database        (P0) [✓]  deps: none
#2  Implement API         (P1) [▶]  deps: #1
#3  Write tests           (P1) [ ]  deps: #1
#4  Deploy to staging     (P2) [ ]  deps: #2, #3
#5  Update docs           (P3) [-]  deps: none
```
