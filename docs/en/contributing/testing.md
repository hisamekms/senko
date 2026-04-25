# Testing

senko has two layers of tests:

1. **Unit tests** — Rust `#[test]` (domain logic, value objects, ...).
2. **E2E tests** — `tests/e2e/*.sh` (exercise the CLI to verify behavior).

## Running

```bash
mise test          # unit + doc tests
mise run e2e       # end-to-end
```

> **Rule**: don't use `cargo test` / `bash tests/e2e/run.sh` directly — always go through the `mise` tasks. mise sets up env vars, the embedded PostgreSQL, etc.

### Running a subset

mise tasks forward extra args:

```bash
mise test task::tests::                    # a specific module
mise test -- --nocapture                   # show println! output
mise run e2e:sqlite -- --fast              # skip watch_* tests for a faster run
```

## Writing Unit Tests

Inside each module's `mod tests`. Domain-layer value objects and status transitions are the heartland:

```rust
// src/domain/task.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_to_todo_is_allowed() {
        let t = Task::new("x");
        let t = t.ready().unwrap();
        assert_eq!(t.status, Status::Todo);
    }

    #[test]
    fn cannot_complete_from_draft() {
        let t = Task::new("x");
        assert!(t.complete().is_err());
    }
}
```

- **domain**: pure state transitions / validation.
- **application**: exercise services using mocks / stubs for ports.
- **infra**: integration tests against SQLite (using a tempfile DB).

## E2E Tests

Shell scripts drive the **real binary** to verify behavior. Common helpers live in `tests/e2e/helpers.sh`.

### Shape

```bash
#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

setup_fresh_project   # tempdir with .senko
trap cleanup EXIT

senko task add --title hello
out=$(senko task list)
assert_json "$out" '.[] | select(.title=="hello")'
```

### Key helpers

| Function | Purpose |
|---|---|
| `setup_fresh_project` | Fresh tempdir + project root |
| `start_serve` / `stop_serve` | Launch/stop `senko serve` in background |
| `assert_eq` / `assert_json` | Result assertions |
| `cleanup` | Remove tempdir |

### E2E against PostgreSQL

```bash
mise run e2e:postgres          # runs the suite against an embedded Postgres only
```

`postgresql_embedded` dev-dependency downloads a JVM and spins up a temporary Postgres. Slow — use selectively locally.

### E2E against the HTTP backend

These tests launch `serve` internally and drive the CLI over HTTP. They are part of the regular `mise run e2e` run:

```
test_http_backend.sh
test_serve_api.sh
test_http_hooks.sh
```

### Auth-related E2E

```bash
test_api_keys.sh
test_auth_session.sh
test_auth_token.sh
test_token_relay.sh
test_trusted_headers.sh
```

## CI

GitHub Actions runs:

- `cargo fmt --check`
- `cargo clippy --all-features --all-targets -- -D warnings`
- `cargo test --all-features`
- `mise run e2e` (SQLite + HTTP)

PRs trigger these automatically. Reproduce and fix locally on failure.

## Coverage Goals

- Domain layer: 100% on the main paths.
- Application layer: happy paths + major error paths.
- Infra layer: representative queries (list / filter / transaction).
- E2E: **new features must have e2e coverage** (it's the user-observable surface).

## Test-Writing Guidelines

- **One observation per test** — don't check multiple things in a single `#[test]`.
- **E2E mimics "what a human would do at the CLI"** — don't touch internals; judge from CLI output.
- **Be careful with time-dependent tests** — don't use `chrono::Utc::now()` directly; design so tests can inject a `fn now() -> DateTime<Utc>`.
- **Make failure diffs readable** — use `assert_eq!(got, expected)` in that argument order.
