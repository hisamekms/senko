//! Deterministic sample-data fixtures.
//!
//! Hand-written so re-running the seeder produces the same shape of data:
//! 3 extra users, 5 contracts (with notes and DoD), 60 tasks across all
//! statuses with 30 dependency edges. A second project ("[seed] Secondary")
//! with the same 4 members and 3 small draft tasks is also created so the
//! web project switcher always has something to switch between. No randomness
//! anywhere — only the `created_at` timestamp drifts (it comes from the
//! database default, which is "now").

use anyhow::Result;

use crate::application::port::TaskBackend;
use crate::domain::DEFAULT_PROJECT_ID;
use crate::domain::contract::{ContractId, ContractNote, CreateContractParams};
use crate::domain::project::CreateProjectParams;
use crate::domain::task::{
    AssigneeUserId, CreateTaskParams, DodItem, Priority, Task, TaskId, TaskStatus,
};
use crate::domain::user::{AddProjectMemberParams, CreateUserParams, Role, UserId, Username};

const SEED_TAG: &str = "seed";

/// Top-level entrypoint: load every fixture into the backend.
pub async fn load(backend: &dyn TaskBackend) -> Result<()> {
    let user_ids = create_users(backend).await?;
    let contract_ids = create_contracts(backend).await?;
    let task_ids = create_tasks(backend, &contract_ids, &user_ids).await?;
    apply_task_states(backend, &task_ids, &contract_ids, &user_ids).await?;
    add_contract_notes(backend, &contract_ids, &task_ids).await?;
    create_secondary_project(backend, &user_ids).await?;
    Ok(())
}

// --- users ---

async fn create_users(backend: &dyn TaskBackend) -> Result<Vec<UserId>> {
    let people = [
        ("alice", "Alice Sample", "alice@example.com"),
        ("bob", "Bob Sample", "bob@example.com"),
        ("carol", "Carol Sample", "carol@example.com"),
    ];
    let mut ids = Vec::with_capacity(people.len());
    for (uname, display, email) in people {
        let username = Username::try_from(uname.to_string())?;
        let user = backend
            .create_user(&CreateUserParams {
                username,
                sub: None,
                display_name: Some(display.to_string()),
                email: Some(email.to_string()),
            })
            .await?;
        backend
            .add_project_member(
                DEFAULT_PROJECT_ID,
                &AddProjectMemberParams {
                    user_id: user.id(),
                    role: Role::Member,
                },
            )
            .await?;
        ids.push(user.id());
    }
    Ok(ids)
}

// --- contracts ---

struct ContractFixture {
    title: &'static str,
    description: &'static str,
    dod: &'static [(&'static str, bool)],
}

const CONTRACTS: &[ContractFixture] = &[
    ContractFixture {
        title: "[seed] Auth refactor",
        description: "Migrate the legacy session middleware to a token-based flow.",
        dod: &[
            ("Spike + decision doc reviewed", true),
            ("Token issuance endpoint shipped", true),
            ("Legacy middleware removed", false),
        ],
    },
    ContractFixture {
        title: "[seed] Search v2",
        description: "Replace the LIKE-based search with a proper query layer.",
        dod: &[
            ("Index design documented", true),
            ("Query parser merged", false),
            ("E2E coverage added", false),
        ],
    },
    ContractFixture {
        title: "[seed] Notification pipeline",
        description: "Outbox-pattern delivery for email and webhook notifications.",
        dod: &[
            ("Outbox table migration", true),
            ("Email channel live", true),
        ],
    },
    ContractFixture {
        title: "[seed] Admin dashboard",
        description: "Internal-only dashboard surfacing operational metrics.",
        dod: &[
            ("Dashboard scaffolding", false),
            ("Metric: open tasks per priority", false),
            ("Metric: contract progress", false),
        ],
    },
    ContractFixture {
        title: "[seed] Migrations cleanup",
        description: "Drop dead-letter columns and consolidate duplicate indexes.",
        dod: &[("Audit complete", true)],
    },
];

async fn create_contracts(backend: &dyn TaskBackend) -> Result<Vec<ContractId>> {
    let mut ids = Vec::with_capacity(CONTRACTS.len());
    for fx in CONTRACTS {
        let dod_strings: Vec<String> = fx.dod.iter().map(|(s, _)| (*s).to_string()).collect();
        let contract = backend
            .create_contract(
                DEFAULT_PROJECT_ID,
                &CreateContractParams {
                    title: fx.title.to_string(),
                    description: Some(fx.description.to_string()),
                    definition_of_done: dod_strings,
                    tags: vec![SEED_TAG.to_string()],
                    metadata: None,
                },
            )
            .await?;
        // Mark requested DoD items as checked. `check_dod` uses 1-based
        // indexing per the contract aggregate.
        for (idx, (_, checked)) in fx.dod.iter().enumerate() {
            if *checked {
                backend.check_dod(contract.id(), idx + 1).await?;
            }
        }
        ids.push(contract.id());
    }
    Ok(ids)
}

// --- tasks ---

#[derive(Clone, Copy)]
struct TaskFixture {
    title: &'static str,
    description: &'static str,
    status: TaskStatus,
    priority: Priority,
    /// Index into `CONTRACTS` (0-based) or None for standalone tasks.
    contract_idx: Option<usize>,
    /// Index into `users` (0 = default user, 1..=3 = alice/bob/carol).
    assignee_idx: usize,
    extra_tag: &'static str,
    dod: &'static [&'static str],
}

/// 60 task fixtures. Status distribution: 3 draft / 30 todo / 8 in_progress /
/// 15 completed / 4 canceled. 25 of them link to contracts (5 per contract).
/// Tasks 1..=15 land on contract 0, 16..=20 on contract 1, etc. — see the
/// `contract_idx` column.
const TASKS: &[TaskFixture] = &[
    // === Contract 0 (Auth refactor) — 6 tasks ===
    TaskFixture {
        title: "[seed] Inventory existing auth endpoints",
        description: "Catalogue every route guarded by the legacy middleware.",
        status: TaskStatus::Completed,
        priority: Priority::P1,
        contract_idx: Some(0),
        assignee_idx: 1,
        extra_tag: "auth",
        dod: &["List endpoints", "Cross-check tests"],
    },
    TaskFixture {
        title: "[seed] Spike: token format options",
        description: "Compare JWT / opaque tokens and write up trade-offs.",
        status: TaskStatus::Completed,
        priority: Priority::P1,
        contract_idx: Some(0),
        assignee_idx: 1,
        extra_tag: "auth",
        dod: &["Spike doc"],
    },
    TaskFixture {
        title: "[seed] Implement token issuance endpoint",
        description: "POST /auth/token returning a signed access token.",
        status: TaskStatus::InProgress,
        priority: Priority::P0,
        contract_idx: Some(0),
        assignee_idx: 2,
        extra_tag: "auth",
        dod: &["Endpoint", "Tests", "Docs"],
    },
    TaskFixture {
        title: "[seed] Migrate /me handler to token auth",
        description: "Switch the canary endpoint to the new path before fan-out.",
        status: TaskStatus::Todo,
        priority: Priority::P1,
        contract_idx: Some(0),
        assignee_idx: 2,
        extra_tag: "auth",
        dod: &["Handler updated", "Backwards-compat shim documented"],
    },
    TaskFixture {
        title: "[seed] Roll out token auth to remaining endpoints",
        description: "Fan-out migration; one PR per endpoint group.",
        status: TaskStatus::Todo,
        priority: Priority::P1,
        contract_idx: Some(0),
        assignee_idx: 0,
        extra_tag: "auth",
        dod: &["All endpoints flipped", "Smoke tested"],
    },
    TaskFixture {
        title: "[seed] Remove legacy session middleware",
        description: "Once every endpoint is on tokens, delete the old code.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: Some(0),
        assignee_idx: 0,
        extra_tag: "auth",
        dod: &["Code deleted", "Schema migration"],
    },
    // === Contract 1 (Search v2) — 6 tasks ===
    TaskFixture {
        title: "[seed] Survey current LIKE-based search usage",
        description: "Identify hot queries, regressions to avoid.",
        status: TaskStatus::Completed,
        priority: Priority::P2,
        contract_idx: Some(1),
        assignee_idx: 3,
        extra_tag: "search",
        dod: &["Usage report"],
    },
    TaskFixture {
        title: "[seed] Design query language grammar",
        description: "Decide tokenizer + parser approach.",
        status: TaskStatus::Completed,
        priority: Priority::P1,
        contract_idx: Some(1),
        assignee_idx: 3,
        extra_tag: "search",
        dod: &["Grammar BNF", "Examples"],
    },
    TaskFixture {
        title: "[seed] Build the FTS index migration",
        description: "Add SQLite FTS5 / Postgres tsvector tables.",
        status: TaskStatus::InProgress,
        priority: Priority::P1,
        contract_idx: Some(1),
        assignee_idx: 1,
        extra_tag: "search",
        dod: &["Migration", "Backfill script"],
    },
    TaskFixture {
        title: "[seed] Write the query parser",
        description: "Tokenizer + AST + simple optimizer.",
        status: TaskStatus::Todo,
        priority: Priority::P1,
        contract_idx: Some(1),
        assignee_idx: 1,
        extra_tag: "search",
        dod: &["Parser", "Unit tests"],
    },
    TaskFixture {
        title: "[seed] Hook search v2 behind a feature flag",
        description: "Gate UX rollout while parity is being verified.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: Some(1),
        assignee_idx: 2,
        extra_tag: "search",
        dod: &["Flag wired", "Default off"],
    },
    TaskFixture {
        title: "[seed] Cut over /search endpoint",
        description: "Once parity is verified, route prod traffic to v2.",
        status: TaskStatus::Draft,
        priority: Priority::P0,
        contract_idx: Some(1),
        assignee_idx: 2,
        extra_tag: "search",
        dod: &["Cutover plan", "Rollback rehearsed"],
    },
    // === Contract 2 (Notification pipeline) — 5 tasks ===
    TaskFixture {
        title: "[seed] Choose outbox storage shape",
        description: "Decide on table layout and ordering invariants.",
        status: TaskStatus::Completed,
        priority: Priority::P1,
        contract_idx: Some(2),
        assignee_idx: 1,
        extra_tag: "notify",
        dod: &["Schema decision"],
    },
    TaskFixture {
        title: "[seed] Outbox table migration",
        description: "DDL + indexes for at-least-once delivery.",
        status: TaskStatus::Completed,
        priority: Priority::P1,
        contract_idx: Some(2),
        assignee_idx: 1,
        extra_tag: "notify",
        dod: &["Migration applied", "Indexes verified"],
    },
    TaskFixture {
        title: "[seed] Email channel adapter",
        description: "SMTP-backed publisher with retries.",
        status: TaskStatus::Completed,
        priority: Priority::P2,
        contract_idx: Some(2),
        assignee_idx: 2,
        extra_tag: "notify",
        dod: &["Adapter shipped", "Retry tests"],
    },
    TaskFixture {
        title: "[seed] Webhook channel adapter",
        description: "HMAC-signed POST with exponential backoff.",
        status: TaskStatus::InProgress,
        priority: Priority::P1,
        contract_idx: Some(2),
        assignee_idx: 3,
        extra_tag: "notify",
        dod: &["Adapter", "Signing tests"],
    },
    TaskFixture {
        title: "[seed] Drain stuck items dashboard",
        description: "Operational view to triage failed deliveries.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: Some(2),
        assignee_idx: 0,
        extra_tag: "notify",
        dod: &["Dashboard", "Drain runbook"],
    },
    // === Contract 3 (Admin dashboard) — 4 tasks ===
    TaskFixture {
        title: "[seed] Pick the dashboard frontend stack",
        description: "Reuse existing web app vs. standalone.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: Some(3),
        assignee_idx: 0,
        extra_tag: "admin",
        dod: &["Decision recorded"],
    },
    TaskFixture {
        title: "[seed] Wire auth gate for /admin",
        description: "Limit to internal users via the new token claim.",
        status: TaskStatus::Todo,
        priority: Priority::P1,
        contract_idx: Some(3),
        assignee_idx: 2,
        extra_tag: "admin",
        dod: &["Gate enforced", "Tests"],
    },
    TaskFixture {
        title: "[seed] Open tasks per priority widget",
        description: "Stacked bar chart split by P0..P3.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: Some(3),
        assignee_idx: 1,
        extra_tag: "admin",
        dod: &["Query", "Widget"],
    },
    TaskFixture {
        title: "[seed] Contract progress widget",
        description: "Bar of DoD checked / total per contract.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: Some(3),
        assignee_idx: 3,
        extra_tag: "admin",
        dod: &["Query", "Widget", "Tooltip"],
    },
    // === Contract 4 (Migrations cleanup) — 4 tasks ===
    TaskFixture {
        title: "[seed] Audit dead-letter columns",
        description: "Find columns not read anywhere and propose removal.",
        status: TaskStatus::Completed,
        priority: Priority::P3,
        contract_idx: Some(4),
        assignee_idx: 0,
        extra_tag: "cleanup",
        dod: &["Audit doc"],
    },
    TaskFixture {
        title: "[seed] Drop column tasks.legacy_owner",
        description: "Behind a migration; double-write removed first.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: Some(4),
        assignee_idx: 1,
        extra_tag: "cleanup",
        dod: &["Migration", "Verified empty"],
    },
    TaskFixture {
        title: "[seed] Consolidate duplicate task_tags indexes",
        description: "Two indexes with overlapping prefixes — keep one.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: Some(4),
        assignee_idx: 2,
        extra_tag: "cleanup",
        dod: &["EXPLAIN before/after"],
    },
    TaskFixture {
        title: "[seed] Vacuum + analyze post-cleanup",
        description: "Run on a follower first to estimate downtime.",
        status: TaskStatus::Canceled,
        priority: Priority::P3,
        contract_idx: Some(4),
        assignee_idx: 3,
        extra_tag: "cleanup",
        dod: &["Plan", "Window booked"],
    },
    // === Standalone (no contract) — 35 tasks ===
    TaskFixture {
        title: "[seed] Investigate flaky test_user_create",
        description: "Fails ~1% on CI with a race in setup.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: None,
        assignee_idx: 1,
        extra_tag: "flake",
        dod: &["Repro recipe", "Fix or quarantine"],
    },
    TaskFixture {
        title: "[seed] Bump rust toolchain to 1.84",
        description: "Pick up new lints.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 0,
        extra_tag: "chore",
        dod: &["Toolchain bumped", "CI green"],
    },
    TaskFixture {
        title: "[seed] Replace ad-hoc string concatenation in CLI helpstrings",
        description: "Use clap derive features consistently.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 2,
        extra_tag: "chore",
        dod: &["Pattern unified"],
    },
    TaskFixture {
        title: "[seed] Document the contract notes feature",
        description: "Walk-through for users + API reference.",
        status: TaskStatus::InProgress,
        priority: Priority::P2,
        contract_idx: None,
        assignee_idx: 3,
        extra_tag: "docs",
        dod: &["Walk-through", "Reference"],
    },
    TaskFixture {
        title: "[seed] Add e2e test for contract DoD flow",
        description: "End-to-end coverage from add → check → list.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: None,
        assignee_idx: 0,
        extra_tag: "e2e",
        dod: &["Test green on both backends"],
    },
    TaskFixture {
        title: "[seed] Clean up stale `.skip` test attributes",
        description: "Some skips reference issues already closed.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 1,
        extra_tag: "chore",
        dod: &["Skips removed or justified"],
    },
    TaskFixture {
        title: "[seed] Profile cold-start CLI latency",
        description: "Track down the 200ms regression seen on macOS.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: None,
        assignee_idx: 2,
        extra_tag: "perf",
        dod: &["Profile data", "Hot spots"],
    },
    TaskFixture {
        title: "[seed] Use Cow<'_, str> in hot path validators",
        description: "Avoids one allocation per validation call.",
        status: TaskStatus::Draft,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 3,
        extra_tag: "perf",
        dod: &["Refactor", "Bench"],
    },
    TaskFixture {
        title: "[seed] Fix typo in DoD validation error",
        description: "\"definitionof_done\" should be \"definition_of_done\".",
        status: TaskStatus::Completed,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 0,
        extra_tag: "polish",
        dod: &["Error string fixed"],
    },
    TaskFixture {
        title: "[seed] Wire up tracing for the seeder",
        description: "Make it easier to debug fixture loads.",
        status: TaskStatus::Completed,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 1,
        extra_tag: "dev-tooling",
        dod: &["Tracing enabled"],
    },
    TaskFixture {
        title: "[seed] Add config option for default priority",
        description: "Let teams override the P2 default.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 2,
        extra_tag: "config",
        dod: &["Option", "Tests"],
    },
    TaskFixture {
        title: "[seed] Surface per-priority counts in `task list`",
        description: "Counts at the top of the listing.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: None,
        assignee_idx: 3,
        extra_tag: "cli",
        dod: &["Counts shown"],
    },
    TaskFixture {
        title: "[seed] Allow filtering by multiple tags (AND semantics)",
        description: "Currently OR; teams want AND for narrowing.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: None,
        assignee_idx: 0,
        extra_tag: "cli",
        dod: &["Flag", "Tests"],
    },
    TaskFixture {
        title: "[seed] Detect and warn on cyclical task deps at add time",
        description: "Easier to fix at the time of insertion.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: None,
        assignee_idx: 1,
        extra_tag: "validation",
        dod: &["Cycle check", "Tests"],
    },
    TaskFixture {
        title: "[seed] Reject empty task title in CLI",
        description: "API already rejects, CLI lets it through.",
        status: TaskStatus::Completed,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 2,
        extra_tag: "validation",
        dod: &["CLI rejects"],
    },
    TaskFixture {
        title: "[seed] Add machine-readable error codes to the API",
        description: "Frontend wants to localize error messages.",
        status: TaskStatus::Todo,
        priority: Priority::P1,
        contract_idx: None,
        assignee_idx: 3,
        extra_tag: "api",
        dod: &["Codes assigned", "Reference doc"],
    },
    TaskFixture {
        title: "[seed] Stream task list output for large projects",
        description: "Avoid buffering the entire result.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: None,
        assignee_idx: 0,
        extra_tag: "perf",
        dod: &["Streaming", "Memory bench"],
    },
    TaskFixture {
        title: "[seed] Permit absolute paths in --plan-file",
        description: "Currently rejected as an error.",
        status: TaskStatus::Completed,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 1,
        extra_tag: "cli",
        dod: &["Accept", "Tests"],
    },
    TaskFixture {
        title: "[seed] Write a CONTRIBUTING.md",
        description: "Linkable guide for new contributors.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 2,
        extra_tag: "docs",
        dod: &["File present", "Linked from README"],
    },
    TaskFixture {
        title: "[seed] Add a `senko doctor` command",
        description: "Self-test for installation and config.",
        status: TaskStatus::Draft,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 3,
        extra_tag: "cli",
        dod: &["Command", "Checks"],
    },
    TaskFixture {
        title: "[seed] Track flaky e2e suite over time",
        description: "Histogram of failure modes.",
        status: TaskStatus::Canceled,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 0,
        extra_tag: "e2e",
        dod: &["Tracker"],
    },
    TaskFixture {
        title: "[seed] Update keyring backend on Linux",
        description: "Old version had a kernel-side leak.",
        status: TaskStatus::Completed,
        priority: Priority::P2,
        contract_idx: None,
        assignee_idx: 1,
        extra_tag: "deps",
        dod: &["Bumped", "CI green"],
    },
    TaskFixture {
        title: "[seed] Audit OAuth scopes requested by senko login",
        description: "Trim the list to the minimum.",
        status: TaskStatus::Todo,
        priority: Priority::P1,
        contract_idx: None,
        assignee_idx: 2,
        extra_tag: "security",
        dod: &["Audit", "Trim PR"],
    },
    TaskFixture {
        title: "[seed] Encrypt API key at rest in the keyring",
        description: "Currently base64; needs symmetric key.",
        status: TaskStatus::Todo,
        priority: Priority::P1,
        contract_idx: None,
        assignee_idx: 3,
        extra_tag: "security",
        dod: &["Encryption", "Migration"],
    },
    TaskFixture {
        title: "[seed] Investigate sqlx 0.9 upgrade",
        description: "API changes; need to estimate effort.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 0,
        extra_tag: "deps",
        dod: &["Estimate doc"],
    },
    TaskFixture {
        title: "[seed] Add Postgres advisory locks for migrations",
        description: "Avoid double-runs in HA deploys.",
        status: TaskStatus::InProgress,
        priority: Priority::P1,
        contract_idx: None,
        assignee_idx: 1,
        extra_tag: "infra",
        dod: &["Lock taken", "Test under contention"],
    },
    TaskFixture {
        title: "[seed] Document supported Postgres versions",
        description: "Currently undocumented; tests use 16.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 2,
        extra_tag: "docs",
        dod: &["Doc"],
    },
    TaskFixture {
        title: "[seed] Add JSON schema for the config file",
        description: "Editors can offer completion / validation.",
        status: TaskStatus::Todo,
        priority: Priority::P2,
        contract_idx: None,
        assignee_idx: 3,
        extra_tag: "tooling",
        dod: &["Schema", "Published"],
    },
    TaskFixture {
        title: "[seed] Fix flaky e2e: `task complete` on slow disk",
        description: "Timeout fires before fsync settles.",
        status: TaskStatus::Canceled,
        priority: Priority::P2,
        contract_idx: None,
        assignee_idx: 0,
        extra_tag: "flake",
        dod: &["Repro", "Fix"],
    },
    TaskFixture {
        title: "[seed] Pretty-print JSON on `--output text`",
        description: "Indent + sort keys for readability.",
        status: TaskStatus::InProgress,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 1,
        extra_tag: "cli",
        dod: &["Pretty"],
    },
    TaskFixture {
        title: "[seed] Add `senko whoami` command",
        description: "Show current user / API key context.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 2,
        extra_tag: "cli",
        dod: &["Command", "Tests"],
    },
    TaskFixture {
        title: "[seed] Write architecture overview",
        description: "Diagram + walkthrough for new joiners.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 3,
        extra_tag: "docs",
        dod: &["Doc", "Diagram"],
    },
    TaskFixture {
        title: "[seed] Reduce initial allocations in the parser",
        description: "Profile says 30% of startup is parser.",
        status: TaskStatus::Canceled,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 0,
        extra_tag: "perf",
        dod: &["Bench", "Patch"],
    },
    TaskFixture {
        title: "[seed] Reproduce: lost tag on task move",
        description: "Move from project A to B drops `seed` tag.",
        status: TaskStatus::Todo,
        priority: Priority::P1,
        contract_idx: None,
        assignee_idx: 1,
        extra_tag: "bug",
        dod: &["Repro", "Fix"],
    },
    TaskFixture {
        title: "[seed] Add CHANGELOG.md",
        description: "Per-release notes; first entry is 0.42.",
        status: TaskStatus::Todo,
        priority: Priority::P3,
        contract_idx: None,
        assignee_idx: 2,
        extra_tag: "docs",
        dod: &["File", "Workflow"],
    },
];

const _: () = {
    // Compile-time assertion that we have exactly 60 task fixtures.
    assert!(TASKS.len() == 60);
};

async fn create_tasks(
    backend: &dyn TaskBackend,
    contract_ids: &[ContractId],
    user_ids: &[UserId],
) -> Result<Vec<TaskId>> {
    let mut ids = Vec::with_capacity(TASKS.len());
    for fx in TASKS {
        let dod_strings: Vec<String> = fx.dod.iter().map(|s| (*s).to_string()).collect();
        let assignee = resolve_assignee(fx.assignee_idx, user_ids);
        let contract_id = fx.contract_idx.map(|i| contract_ids[i]);
        let task = backend
            .create_task(
                DEFAULT_PROJECT_ID,
                &CreateTaskParams {
                    title: fx.title.to_string(),
                    background: None,
                    description: Some(fx.description.to_string()),
                    priority: Some(fx.priority),
                    definition_of_done: dod_strings,
                    in_scope: Vec::new(),
                    out_of_scope: Vec::new(),
                    branch: None,
                    pr_url: None,
                    metadata: None,
                    tags: vec![SEED_TAG.to_string(), fx.extra_tag.to_string()],
                    dependencies: Vec::new(),
                    assignee_user_id: Some(assignee),
                    contract_id,
                },
            )
            .await?;
        ids.push(task.id());
    }
    Ok(ids)
}

fn resolve_assignee(idx: usize, user_ids: &[UserId]) -> AssigneeUserId {
    // idx 0 = default user (id=1), 1..=3 = the three seeded users
    if idx == 0 {
        AssigneeUserId::Id(crate::domain::DEFAULT_USER_ID)
    } else {
        AssigneeUserId::Id(user_ids[idx - 1])
    }
}

// --- task state + dependencies ---

/// 30 dependency edges. Each pair `(downstream_idx, upstream_idx)` says
/// task TASKS[downstream] depends on task TASKS[upstream]. The upstream is
/// always smaller-indexed than the downstream so the graph is acyclic.
const DEPS: &[(usize, usize)] = &[
    // Auth refactor chain
    (1, 0),
    (2, 1),
    (3, 2),
    (4, 2),
    (5, 3),
    (5, 4),
    // Search v2 chain
    (7, 6),
    (8, 7),
    (9, 8),
    (10, 9),
    (11, 10),
    // Notification pipeline chain
    (13, 12),
    (14, 12),
    (15, 14),
    (16, 14),
    // Admin dashboard chain
    (18, 17),
    (19, 17),
    (20, 17),
    // Migrations cleanup
    (22, 21),
    (23, 22),
    (24, 23),
    // Cross-contract / standalone interplay
    (28, 24), // docs → cleanup audit
    (35, 32), // surface per-priority counts → wire up tracing
    (40, 35), // detect cyclical deps → counts widget
    (45, 25), // audit OAuth scopes → flaky test
    (52, 25), // documenting Postgres versions → flaky test
    (55, 50), // pretty-print JSON → architecture overview
    (57, 33), // CHANGELOG → contract progress widget
    (29, 26), // permit abs paths → trim ad-hoc strings
    (43, 35), // migration advisory locks depends on tracing
];

/// Hand-picked timestamps so the seeded data has a coherent timeline.
const STARTED_AT_BASE: &str = "2026-04-15T10:00:00Z";
const COMPLETED_AT_BASE: &str = "2026-04-20T15:30:00Z";
const CANCELED_AT_BASE: &str = "2026-04-22T11:45:00Z";

async fn apply_task_states(
    backend: &dyn TaskBackend,
    task_ids: &[TaskId],
    contract_ids: &[ContractId],
    user_ids: &[UserId],
) -> Result<()> {
    // Pre-build a per-task dependency list keyed by index so each save call
    // can include the right set in one shot.
    let mut deps_by_idx: Vec<Vec<TaskId>> = vec![Vec::new(); TASKS.len()];
    for (down, up) in DEPS {
        deps_by_idx[*down].push(task_ids[*up]);
    }

    for (i, fx) in TASKS.iter().enumerate() {
        // Draft tasks are already in the right state from `create_task` and
        // they take no dependencies in this seed set; skip the save round-trip.
        let needs_deps = !deps_by_idx[i].is_empty();
        let needs_status_change = fx.status != TaskStatus::Draft;
        // Half of each task's DoD items are checked, picked deterministically
        // by index parity. Completed tasks need ALL items checked because the
        // domain enforces that on transition — we honor that even though we
        // bypass the transition (we still want fixtures that look right).
        let dod = build_dod(fx);
        if !needs_deps && !needs_status_change && !dod.iter().any(DodItem::checked) {
            continue;
        }
        let task = construct_task(
            i,
            fx,
            task_ids,
            contract_ids,
            user_ids,
            &deps_by_idx[i],
            dod,
        );
        backend.save(&task).await?;
    }
    Ok(())
}

fn build_dod(fx: &TaskFixture) -> Vec<DodItem> {
    let total = fx.dod.len();
    fx.dod
        .iter()
        .enumerate()
        .map(|(idx, content)| {
            // Completed tasks: every item checked. Otherwise alternate, with
            // earlier items checked first to mimic real progress.
            let checked = if fx.status == TaskStatus::Completed {
                true
            } else {
                idx < total / 2
            };
            DodItem::new((*content).to_string(), checked)
        })
        .collect()
}

fn construct_task(
    idx: usize,
    fx: &TaskFixture,
    task_ids: &[TaskId],
    contract_ids: &[ContractId],
    user_ids: &[UserId],
    deps: &[TaskId],
    dod: Vec<DodItem>,
) -> Task {
    use crate::domain::DEFAULT_USER_ID;

    let id = task_ids[idx];
    let project_id = DEFAULT_PROJECT_ID;
    let started_at = match fx.status {
        TaskStatus::InProgress | TaskStatus::Completed => Some(STARTED_AT_BASE.to_string()),
        _ => None,
    };
    let completed_at = match fx.status {
        TaskStatus::Completed => Some(COMPLETED_AT_BASE.to_string()),
        _ => None,
    };
    let (canceled_at, cancel_reason) = match fx.status {
        TaskStatus::Canceled => (
            Some(CANCELED_AT_BASE.to_string()),
            Some("[seed] cancelled because the originating need was de-prioritised".to_string()),
        ),
        _ => (None, None),
    };
    let assignee_user_id = if fx.assignee_idx == 0 {
        Some(DEFAULT_USER_ID)
    } else {
        Some(user_ids[fx.assignee_idx - 1])
    };
    let contract_id = fx.contract_idx.map(|i| contract_ids[i]);
    // updated_at: save reads this verbatim into the row. created_at is left
    // untouched by save, so it keeps the database default ("now").
    let updated_at = completed_at
        .clone()
        .or_else(|| canceled_at.clone())
        .or_else(|| started_at.clone())
        .unwrap_or_else(|| STARTED_AT_BASE.to_string());
    Task::new(
        id,
        project_id,
        fx.title.to_string(),
        None,
        Some(fx.description.to_string()),
        None,
        fx.priority,
        fx.status,
        None,
        assignee_user_id,
        STARTED_AT_BASE.to_string(),
        updated_at,
        started_at,
        completed_at,
        canceled_at,
        cancel_reason,
        None,
        None,
        contract_id,
        None,
        dod,
        Vec::new(),
        Vec::new(),
        vec![SEED_TAG.to_string(), fx.extra_tag.to_string()],
        deps.to_vec(),
    )
}

// --- contract notes ---

struct NoteFixture {
    contract_idx: usize,
    /// Index into TASKS — the task the note was generated from. Or None.
    source_task_idx: Option<usize>,
    content: &'static str,
    created_at: &'static str,
}

const NOTES: &[NoteFixture] = &[
    NoteFixture {
        contract_idx: 0,
        source_task_idx: Some(0),
        content: "Inventory turned up 4 endpoints we didn't know were guarded.",
        created_at: "2026-04-02T09:15:00Z",
    },
    NoteFixture {
        contract_idx: 0,
        source_task_idx: Some(1),
        content: "Picking opaque tokens for v1 — JWT can land later if we need it.",
        created_at: "2026-04-04T14:00:00Z",
    },
    NoteFixture {
        contract_idx: 0,
        source_task_idx: Some(2),
        content: "Token endpoint shipped behind a flag; rolling to internal users next week.",
        created_at: "2026-04-18T11:30:00Z",
    },
    NoteFixture {
        contract_idx: 1,
        source_task_idx: Some(6),
        content: "LIKE-based search is hot on three queries; those become parity tests.",
        created_at: "2026-04-03T10:00:00Z",
    },
    NoteFixture {
        contract_idx: 1,
        source_task_idx: Some(7),
        content: "Grammar reviewed. Reserved-word collision with `tag:` resolved by escaping.",
        created_at: "2026-04-08T16:20:00Z",
    },
    NoteFixture {
        contract_idx: 1,
        source_task_idx: None,
        content: "Reminder: search v2 cutover blocks on auth refactor — tokens carry the user-id claim used for ACL filtering.",
        created_at: "2026-04-19T09:00:00Z",
    },
    NoteFixture {
        contract_idx: 2,
        source_task_idx: Some(12),
        content: "Outbox table will use a monotonic sequence id; ordering invariant doc'd.",
        created_at: "2026-04-05T08:45:00Z",
    },
    NoteFixture {
        contract_idx: 2,
        source_task_idx: Some(13),
        content: "Migration applied to staging; ~120ms on the largest tenant.",
        created_at: "2026-04-07T13:10:00Z",
    },
    NoteFixture {
        contract_idx: 2,
        source_task_idx: Some(14),
        content: "Email channel went green. SES is the prod transport; SMTP is the dev fallback.",
        created_at: "2026-04-12T17:00:00Z",
    },
    NoteFixture {
        contract_idx: 3,
        source_task_idx: None,
        content: "Deferred until auth gate exists — admin views must not leak metrics to non-internal users.",
        created_at: "2026-04-16T08:00:00Z",
    },
    NoteFixture {
        contract_idx: 3,
        source_task_idx: Some(17),
        content: "Decided to embed admin dashboard inside the existing web app rather than a standalone deploy.",
        created_at: "2026-04-21T15:30:00Z",
    },
    NoteFixture {
        contract_idx: 4,
        source_task_idx: Some(21),
        content: "Audit finished. 4 dead-letter columns, 2 redundant indexes.",
        created_at: "2026-04-09T11:00:00Z",
    },
    NoteFixture {
        contract_idx: 4,
        source_task_idx: None,
        content: "Plan: do drops in two waves so observability stays cheap to roll back.",
        created_at: "2026-04-10T10:30:00Z",
    },
    NoteFixture {
        contract_idx: 0,
        source_task_idx: None,
        content: "Kicking off rollout next sprint — token middleware passes load test (3k rps).",
        created_at: "2026-04-25T09:30:00Z",
    },
    NoteFixture {
        contract_idx: 2,
        source_task_idx: Some(15),
        content: "Webhook adapter signing decided: HMAC-SHA256 over canonical JSON; key per-tenant.",
        created_at: "2026-04-26T14:20:00Z",
    },
];

async fn add_contract_notes(
    backend: &dyn TaskBackend,
    contract_ids: &[ContractId],
    task_ids: &[TaskId],
) -> Result<()> {
    for nf in NOTES {
        let source_task = nf.source_task_idx.map(|i| task_ids[i]);
        backend
            .add_note(
                contract_ids[nf.contract_idx],
                &ContractNote::new(
                    nf.content.to_string(),
                    source_task,
                    nf.created_at.to_string(),
                ),
            )
            .await?;
    }
    Ok(())
}

// --- secondary project ---

const SECONDARY_TASKS: &[(&str, &str, &str)] = &[
    (
        "[seed] Draft a roadmap for the secondary workstream",
        "Outline the goals so the team can prioritise.",
        "planning",
    ),
    (
        "[seed] Set up a parking-lot of cross-team requests",
        "Triage incoming asks before they pile up.",
        "triage",
    ),
    (
        "[seed] Schedule the kick-off review",
        "Pick a time that works across timezones.",
        "ops",
    ),
];

/// Create a second project so the web project switcher always has at least
/// two entries to switch between. The bootstrap user (DEFAULT_USER_ID) is
/// added as Owner and the three seeded users (alice/bob/carol) as Members,
/// mirroring the membership of the primary project.
async fn create_secondary_project(backend: &dyn TaskBackend, user_ids: &[UserId]) -> Result<()> {
    use crate::domain::DEFAULT_USER_ID;

    let project = backend
        .create_project(&CreateProjectParams {
            name: "[seed] Secondary".to_string(),
            description: Some(
                "Auxiliary project surfaced by the seeder so the web project switcher has \
                 something to switch between."
                    .to_string(),
            ),
        })
        .await?;
    let project_id = project.id();

    backend
        .add_project_member(
            project_id,
            &AddProjectMemberParams {
                user_id: DEFAULT_USER_ID,
                role: Role::Owner,
            },
        )
        .await?;
    for &uid in user_ids {
        backend
            .add_project_member(
                project_id,
                &AddProjectMemberParams {
                    user_id: uid,
                    role: Role::Member,
                },
            )
            .await?;
    }

    for (title, description, extra_tag) in SECONDARY_TASKS {
        backend
            .create_task(
                project_id,
                &CreateTaskParams {
                    title: (*title).to_string(),
                    background: None,
                    description: Some((*description).to_string()),
                    priority: Some(Priority::P2),
                    definition_of_done: Vec::new(),
                    in_scope: Vec::new(),
                    out_of_scope: Vec::new(),
                    branch: None,
                    pr_url: None,
                    metadata: None,
                    tags: vec![SEED_TAG.to_string(), (*extra_tag).to_string()],
                    dependencies: Vec::new(),
                    assignee_user_id: Some(AssigneeUserId::Id(DEFAULT_USER_ID)),
                    contract_id: None,
                },
            )
            .await?;
    }

    Ok(())
}
