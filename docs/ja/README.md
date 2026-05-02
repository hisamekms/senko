# senko ドキュメント

senko は **AI エージェントが自律的に作業を進めるためのワークフローオーケストレータ** です。
「タスク管理ツール」というより、**プロジェクト固有の進め方を codify してエージェントに教える** 道具に近い位置づけで、Claude Code と連携して使うことを主眼に設計されています。

> **日本語 (このディレクトリ)** / [English](../../README.md)

## コアコンセプト: 3 つの柱

senko は AI エージェントの自律的動作を、以下 3 つの柱で支えます。

1. **イベントドリブンなワークフロー** — プロジェクト固有のルール (DoD / ブランチ規則 / 必須 metadata / 段階ごとの指示) を、エージェントの行動に合わせて自動で注入・検証する。hook と workflow stage がこの役割を担う
2. **「次の 1 件」に集中できる実行モデル** — 大きな作業を依存関係と優先度を持つタスクに分割し、エージェントは常に「次の 1 件」だけに取り組む。ワンショットの巨大プロンプトに詰め込まず、タスクごとに context をリセットできる。依存が解けた複数タスクは複数 session から並列 pick も可能
3. **Contract で全体像を保持** — タスクは 1 つの context window 内で完結・破棄される粒度だが、Contract は **複数タスクを束ねる粒度** で継続し、Notes とともに横断的な文脈と知見を保持する。タスクが増えても作業の全体像を見失わない

→ 深掘り: [コアコンセプト: 3 つの柱](explanation/core-concept.md)

## 30 秒で試す

```bash
# 1. バイナリをインストール
curl -fsSL https://raw.githubusercontent.com/hisamekms/senko/main/install.sh | sh

# 2. プロジェクト直下で skill をインストール
cd your-project
senko skill-install

# 3. Claude Code で
#    /senko task add Implement webhook handler
#    /senko
```

## ドキュメント構成

読者の目的別に 4 層に分かれています。

### まず手を動かしたい — [getting-started/](getting-started/)

3 つの典型構成について、概要・構成図・エンドツーエンドのセットアップ手順を示します。

- [ローカル SQLite](getting-started/local-sqlite.md) — 個人開発、1 人で完結
- [CLI → Remote → PostgreSQL](getting-started/cli-remote-postgres.md) — チームでサーバを共有
- [CLI → Relay → Remote → PostgreSQL](getting-started/cli-relay-remote-postgres.md) — AI サンドボックス (CLI シークレットレス / Relay にシークレット集約)

### 考え方を理解したい — [explanation/](explanation/)

3 つの柱を軸に、senko が「なぜこう設計されているか」を説明します。

- [コアコンセプト: 3 つの柱](explanation/core-concept.md) — 最初に読む
- [イベントドリブンなワークフロー](explanation/event-driven-workflow.md) — 柱 1: hook と workflow stage の仕組み
- [「次の 1 件」に集中できる実行モデル](explanation/task-decomposition.md) — 柱 2: 依存・優先度・並列 pick
- [Contract による全体像の保持](explanation/contract.md) — 柱 3: 長期文脈と Notes
- [Runtime の使い分け](explanation/runtimes.md) — cli / server.remote / server.relay のデリバリ基盤

### 設定・デプロイ方法を知りたい — [guides/](guides/)

デプロイ形態別に目的の How-To を引きます。

**分散トレーシング** (runtime 横断)
- [OTel Tracing 運用ガイド](guides/tracing.md) — Claude Code との共存、Jaeger / Tempo / console での `event.name` クエリ、Aviary 連携、監査用フィルタ、セキュリティ考慮

**CLI を使う人** — [guides/cli/](guides/cli/)
- [Skill のインストールと更新](guides/cli/skill-install.md)
- [Workflow stage の実例](guides/cli/workflow-stages.md)
- [`[cli.*]` hook の実例](guides/cli/hooks.md)
- [CLI backend の切替](guides/cli/backends.md) — SQLite / PostgreSQL / HTTP

**サーバ運用者 (`senko serve`)** — [guides/server-remote/](guides/server-remote/)
- [デプロイ](guides/server-remote/deploy.md)
- [OIDC 認証](guides/server-remote/auth-oidc.md) — 本番の推奨方式 (人間は PKCE、bot は Client Credentials)
- [信頼ヘッダ認証](guides/server-remote/auth-trusted-headers.md) — API Gateway 配下
- [AWS デプロイ](guides/server-remote/aws-deployment.md) — API Gateway + Cognito + Lambda
- [`[server.remote.*]` hook の実例](guides/server-remote/hooks.md)
- [API キー認証](guides/server-remote/auth-api-key.md) — 動作確認・ブートストラップ用 (本番で積極採用する場面はない)

**リレー運用者 (relay モードで動く `senko serve`)** — [guides/server-relay/](guides/server-relay/)
- [デプロイ](guides/server-relay/deploy.md)
- [トークン中継パターン](guides/server-relay/token-relay.md)
- [`[server.relay.*]` hook の実例](guides/server-relay/hooks.md)

**senko-web 運用者 (TanStack Start SSR)** — [guides/web/](guides/web/README.md)
- [デプロイガイド](guides/web/README.md) — env 変数 / tarball 入手 / デプロイ先一覧
- [AWS Lambda + Amazon Cognito](guides/web/aws-lambda-cognito.md) — v1 で公式サポート

### 仕様を引きたい — [reference/](reference/)

- [CLI リファレンス](reference/cli.md) — サブコマンド全量
- [REST API リファレンス](reference/api.md) — エンドポイント全量
- [データモデル](reference/data-model.md) — DB スキーマ
- [Hooks リファレンス](reference/hooks.md) — envelope / trigger マトリクス
- [Tracing リファレンス](reference/tracing.md) — 業務イベント (`event.name` LogRecord) 一覧、enduser / target / Resource 属性スキーマ、baggage / traceparent 伝搬、Remote OTel 環境変数
- **設定リファレンス** — [reference/config/](reference/config/)
  - [概論](reference/config/overview.md) — ファイル配置・優先順位・runtime フィルタ
  - [`[cli.*]`](reference/config/cli.md)
  - [`[server.remote.*]` / `[backend.*]` / `[server.auth.*]`](reference/config/server-remote.md)
  - [`[server.relay.*]`](reference/config/server-relay.md)
  - [`[workflow.*]`](reference/config/workflow.md)
  - [`[project]` / `[user]` / `[log]`](reference/config/common.md)

### コントリビュート — [contributing/](contributing/)

- [開発環境セットアップ](contributing/development.md)
- [4 層アーキテクチャ](contributing/architecture.md) — コード構造 (domain / application / infra / presentation)
- [テスト](contributing/testing.md) — unit / e2e
- [リリース手順](contributing/releasing.md)
- [Worktree ワークフロー](contributing/worktree.md)

## ライセンス

MIT
