# Summary

[senko ドキュメント](ja/README.md)

---

# はじめに

- [ローカル SQLite](ja/getting-started/local-sqlite.md)
- [CLI → Remote → PostgreSQL](ja/getting-started/cli-remote-postgres.md)
- [CLI → Relay → Remote → PostgreSQL (AI サンドボックス)](ja/getting-started/cli-relay-remote-postgres.md)

# ガイド

- [OTel Tracing 運用](ja/guides/tracing.md)
- [CLI]()
  - [CLI backend を切り替える](ja/guides/cli/backends.md)
  - [`[cli.*]` hook の実例](ja/guides/cli/hooks.md)
  - [Claude Code skill のインストールと更新](ja/guides/cli/skill-install.md)
  - [Workflow stage の実例](ja/guides/cli/workflow-stages.md)
- [Server Relay]()
  - [`senko serve` を relay モードでデプロイ](ja/guides/server-relay/deploy.md)
  - [`[server.relay.*]` hook の実例](ja/guides/server-relay/hooks.md)
  - [トークン中継 (Token Relay) パターン](ja/guides/server-relay/token-relay.md)
- [Server Remote]()
  - [`senko serve` をデプロイ](ja/guides/server-remote/deploy.md)
  - [API キー認証](ja/guides/server-remote/auth-api-key.md)
  - [OIDC 認証](ja/guides/server-remote/auth-oidc.md)
  - [信頼ヘッダ (trusted_headers) 認証](ja/guides/server-remote/auth-trusted-headers.md)
  - [Dev Bypass 認証 (開発専用)](ja/guides/server-remote/auth-dev-bypass.md)
  - [AWS デプロイ (API Gateway + Cognito + Lambda Web Adapter)](ja/guides/server-remote/aws-deployment.md)
  - [`[server.remote.*]` hook の実例](ja/guides/server-remote/hooks.md)

# リファレンス

- [REST API](ja/reference/api.md)
- [CLI](ja/reference/cli.md)
- [データモデル](ja/reference/data-model.md)
- [Hooks](ja/reference/hooks.md)
- [Tracing](ja/reference/tracing.md)
- [設定ファイル]()
  - [概論](ja/reference/config/overview.md)
  - [`[project]` / `[user]` / `[log]` / `[web]`](ja/reference/config/common.md)
  - [`[cli.*]`](ja/reference/config/cli.md)
  - [`[server.relay.*]`](ja/reference/config/server-relay.md)
  - [`[server.*]` / `[backend.*]` / `[server.auth.*]`](ja/reference/config/server-remote.md)
  - [`[workflow.*]`](ja/reference/config/workflow.md)

# 解説

- [コアコンセプト: 3 つの柱](ja/explanation/core-concept.md)
- [イベントドリブンなワークフロー](ja/explanation/event-driven-workflow.md)
- [「次の 1 件」に集中できる実行モデル](ja/explanation/task-decomposition.md)
- [Contract による全体像の保持](ja/explanation/contract.md)
- [Runtime の使い分け](ja/explanation/runtimes.md)

# コントリビューション

- [4 層アーキテクチャ](ja/contributing/architecture.md)
- [開発環境セットアップ](ja/contributing/development.md)
- [テスト](ja/contributing/testing.md)
- [Worktree ワークフロー](ja/contributing/worktree.md)
- [リリース手順](ja/contributing/releasing.md)

# マイグレーション

- [v0.21.0 → v0.22.0](migration-v0.22.0.md)

# 内部ドキュメント

- [Knowledge]()
  - [4 層アーキテクチャ設計](knowledge/layered-architecture-design.md)
  - [senko skill のフロー](knowledge/senko-skill-flow.md)
  - [Tracing: just-one-rebuilder バグ](knowledge/tracing-just-one-rebuilder-bug.md)
  - [ureq v3 API](knowledge/ureq-v3-api.md)
- [Proposals]()
  - [ファイルベース hooks 登録](proposals/file-based-hooks.md)
- [Architecture Review]()
  - [2026-03-30](arch-review/2026-03-30.md)
