# senko Specification

## Overview

senko is a **local-only task management CLI** designed for single-developer or single-agent workflows.  
It is not a collaboration tool and is intended to be used locally within a project.

Tasks are stored in a SQLite database located in the project directory.

The tool is distributed as a **Rust CLI binary via GitHub Releases** and is typically operated by **Claude Code through a generated skill**.

Default output format is **JSON (AI-oriented)** with optional **human-readable output**.

---

# Installation & Distribution

Binary distribution via GitHub Releases.

The CLI binary name:

```

senko

```

Claude Code integration is provided via:

```

senko skill-install

```

which generates a `SKILL.md` bundle.

---

# Project Structure

Database location:

```

<project_root>/.senko/data.db

```

---

# Project Root Resolution

senko determines the project root as follows:

1. If `--project-root` is provided → use it
2. Search upward for `.senko/`
3. If not found → search for Git repository root
4. If not found → use current directory

---

# Database Schema

## tasks

```

id INTEGER PRIMARY KEY AUTOINCREMENT
title TEXT NOT NULL

background TEXT
description TEXT
plan TEXT

priority INTEGER DEFAULT 2

status TEXT NOT NULL

assignee_session_id TEXT

created_at DATETIME NOT NULL
updated_at DATETIME NOT NULL
started_at DATETIME
completed_at DATETIME

canceled_at DATETIME
cancel_reason TEXT

```

status values:

```

draft
todo
in_progress
completed
canceled

```

priority values:

```

0
1
2 (default)
3

```

---

## task_definition_of_done

```

id INTEGER PRIMARY KEY
task_id INTEGER
content TEXT

```

---

## task_in_scope

```

id INTEGER PRIMARY KEY
task_id INTEGER
content TEXT

```

---

## task_out_of_scope

```

id INTEGER PRIMARY KEY
task_id INTEGER
content TEXT

```

---

## task_tags

```

id INTEGER PRIMARY KEY
task_id INTEGER
tag TEXT

```

---

## task_dependencies

Represents **start dependencies**.

A task becomes startable only when all dependency tasks are `completed`.

```

id INTEGER PRIMARY KEY
task_id INTEGER
depends_on_task_id INTEGER

```

---

# Task Model

Task fields:

```

id
title
definition_of_done[]
background
description
plan
in_scope[]
out_of_scope[]
dependencies[]
priority
status
tags[]
assignee_session_id
created_at
updated_at
started_at
completed_at
canceled_at
cancel_reason

```

---

# Status Transitions

Allowed transitions:

```

draft -> todo
todo -> in_progress
in_progress -> completed

draft -> canceled
todo -> canceled
in_progress -> canceled

```

Forbidden transitions:

```

completed -> *
canceled -> *

draft -> in_progress
todo -> completed
in_progress -> todo

```

Command constraints:

```

next: todo -> in_progress
complete: in_progress -> completed
cancel: only allowed if not completed

```

---

# Dependency Semantics

Dependencies represent **requirements to start a task**.

```

A depends on B

```

Means:

```

B must be completed before A can start

```

A task is **startable** when:

```

status = todo
AND all dependencies are completed

```

---

# Next Task Selection

The `next` command selects a task that satisfies:

```

status = todo
AND dependencies completed

```

Selection order:

1. `priority ASC`
2. `created_at ASC`
3. `id ASC`

After selection:

```

status -> in_progress
started_at -> set
assignee_session_id -> set

```

---

# CLI Commands

```

senko add
senko list
senko get
senko next
senko edit
senko complete
senko cancel
senko deps
senko skill-install

```

---

# Output Modes

Default output:

```

JSON

```

Human readable output:

```

--output text

```

---

# Input Modes

Hybrid input support:

### CLI Flags

Example:

```

senko add 
--title "Implement auth API" 
--definition-of-done "tests pass" 
--definition-of-done "docs updated" 
--in-scope "API implementation" 
--in-scope "unit tests" 
--tag backend 
--tag auth

```

### JSON Input

```

--json '<payload>'

```

or

```

--from-json file.json

```

---

# add

Create a new task.

Example:

```

senko add --title "task"

```

---

# list

List tasks with filters.

Supported filters:

```

--status
--depends-on
--ready

```

---

# get

Retrieve a task by id.

```

senko get <task_id>

```

---

# next

Select and start the next task.

```

senko next --session-id <session_id>

```

Behavior:

```

status -> in_progress
started_at -> set
assignee_session_id -> set

```

---

# edit

Edit task fields.

Scalar fields:

```

--title
--background
--description
--plan
--priority
--status

```

Array fields support **replace / add / remove**.

Replace:

```

--set-tags
--set-definition-of-done
--set-in-scope
--set-out-of-scope

```

Add:

```

--add-tag
--add-definition-of-done
--add-in-scope
--add-out-of-scope

```

Remove:

```

--remove-tag
--remove-definition-of-done
--remove-in-scope
--remove-out-of-scope

```

Dependencies are handled separately via `deps`.

---

# complete

Complete a task.

```

senko complete <task_id>

```

Allowed only when:

```

status = in_progress

```

Effects:

```

status -> completed
completed_at -> set

```

---

# cancel

Cancel a task.

```

senko cancel <task_id>

```

If dependent tasks exist, command fails.

Options:

Remove dependency from dependents:

```

--remove-dependency-from-dependents

```

Replace dependency:

```

--replace-dependency-in-dependents <task_id>

```

Effects:

```

status -> canceled
canceled_at -> set
cancel_reason -> optional

```

---

# deps

Manage dependencies.

Add dependency:

```

senko deps add <task_id> <depends_on_task_id>

```

Remove dependency:

```

senko deps remove <task_id> <depends_on_task_id>

```

Replace dependency:

```

senko deps replace <task_id> <old_dep> <new_dep>

```

---

# skill-install

Generate Claude Code skill configuration.

```

senko skill-install

```

Produces:

```

SKILL.md

```

Used to allow Claude Code to operate senko via CLI.
