# Layered Architecture (4 Layers)

senko's codebase **keeps a function-oriented style while being split into four layers**.

```
presentation → application → domain ← infra
                    ↓              ↑
               port (trait)    impl (struct)
```

- **The domain layer depends on nothing** — it only defines traits (ports).
- **The application layer depends on domain traits** — it doesn't know the implementations.
- **The infra layer implements domain traits** — dependency points toward domain.
- **The presentation layer only calls application services** — it doesn't know about domain / infra internals.

## Directory Mapping

```
src/
├── domain/        Domain models (Task / Contract / Project / User / MetadataField)
│   ├── task.rs
│   ├── contract.rs
│   ├── project.rs
│   ├── user.rs
│   └── metadata_field.rs
│
├── application/   Use cases = procedural orchestration of domain + authorization
│   ├── task_service.rs
│   ├── contract_service.rs
│   ├── project_service.rs
│   ├── user_service.rs
│   ├── hook_trigger.rs
│   ├── auth.rs
│   └── port/      Non-domain traits (hook executor, PR verifier, etc.)
│
├── infra/         Port implementations
│   ├── sqlite/
│   ├── postgres/
│   ├── http/      remote backend (= CLI→server HTTP client)
│   ├── hook/      shell hook executor
│   ├── auth.rs    API key / JWT / trusted headers
│   ├── pr_verifier.rs
│   └── config/
│
└── presentation/  Entry points
    ├── cli/       clap subcommands + handlers
    ├── api/       axum handlers (REST API)
    ├── web.rs     read-only web viewer (HTML rendering)
    └── dto.rs     DTOs at the presentation ⇄ application boundary
```

## Responsibilities per Layer

### Presentation

- **cli**: subcommand definitions; parsing args / env / config (precedence args → env → config → default).
- **api**: Axum handlers that delegate to application services.
- **web**: HTML rendering (read-only).
- **Output format**: `--output json|text` lives here.

### Application

- **Authorization** (project member / role based).
- **Procedural orchestration of the domain** — combining multiple domain operations transactionally.
- **Ports for non-domain concerns** — logger / hook executor / PR verifier, etc.
- **Remote / local switching** — `LocalTaskOperations` and `RemoteTaskOperations` implement the same port.

### Domain

- **Aggregate / entity / value object / domain service.**
- **Repository traits** and other tightly domain-related ports are defined here.
- **State-transition logic** (status transitions / dependency checks / DoD validation).
- Operations on entities belonging to an aggregate go through the aggregate root.

### Infra

- **Port implementations**: SQLite / PostgreSQL repositories, HTTP client, shell hook executor, JWT verifier.
- **External-service drivers**: AWS Secrets Manager, GitHub CLI (PR verify).
- An **inbound dependency** on domain — watch the direction carefully.

## Port / Adapter Map

| Port (trait) | Defined in | Implementations (adapters) |
|---|---|---|
| `TaskOperations` | application | `LocalTaskOperations` (→ repository) / `RemoteTaskOperations` (→ HTTP) |
| `ContractOperations` | application | `LocalContractOperations` / `RemoteContractOperations` |
| `ProjectOperations` / `UserOperations` / `MetadataFieldOperations` | application | Same pattern |
| `TaskBackend` (repository aggregation) | application | `SqliteBackend` / `PostgresBackend` |
| `HookExecutor` | application | `ShellHookExecutor` |
| `HookDataSource` | application | `SqliteBackend` / `RemoteHookDataSource` |
| `PrVerifier` | application | `GhCliPrVerifier` |
| `AuthProvider` | application | `ApiKeyProvider` / `JwtAuthProvider` / `TrustedHeadersAuthProvider` |

## The Role of `bootstrap.rs`

`src/bootstrap.rs` owns **assembling the dependency graph**:

1. `resolve_project_root()` finds the project root.
2. `load_config()` loads config and activates sections matching the runtime.
3. `create_backend()` creates a SQLite or PostgreSQL backend.
4. `create_task_operations()` creates local or HTTP operations.
5. Wires in `HookExecutor` and `AuthProvider` when needed.
6. Hands dependencies to the presentation layer (CLI / API / Web) as `Arc<dyn ...>`.

The rule for the presentation layer is to **get all dependencies through bootstrap** (e.g. `crate::bootstrap::create_task_operations`) and **never import infra directly**.

## Runtime × Backend Matrix

```
                │ Local backend  │ HTTP backend (remote)
────────────────┼────────────────┼──────────────────────
cli             │ SqliteBackend  │ RemoteTaskOperations
                │ PostgresBackend│     (via [cli.remote])
server.remote   │ SqliteBackend  │ ─
                │ PostgresBackend│
server.relay    │ ─              │ RemoteTaskOperations
                │                │     (via [server.relay])
```

## Non-Dependency Rules

These are enforced by arch-review:

- The domain layer must not import application / infra / presentation.
- The application layer must not import infra (except in bootstrap.rs).
- The presentation layer must only call into application — don't import infra / domain directly.
- Ports live in application or domain — **never in infra**.

## References

- [docs/knowledge/layered-architecture-design.md](../../knowledge/layered-architecture-design.md) (design-decision rationale)
- The `/arch-review` skill for automated checking
