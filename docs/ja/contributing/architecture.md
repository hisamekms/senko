# 4 層アーキテクチャ

senko のコードベースは **関数中心スタイルを維持しつつ、4 層のレイヤードアーキテクチャ** に分離されています。

```
presentation → application → domain ← infra
                    ↓              ↑
               port (trait)    impl (struct)
```

- **domain 層はどこにも依存しない** — trait (port) を定義するだけ
- **application 層は domain の trait に依存** — 具体実装は知らない
- **infra 層は domain の trait を実装** — 依存の方向が domain 向き
- **presentation 層は application service を呼ぶだけ** — domain/infra の詳細を知らない

## ディレクトリ対応

```
src/
├── domain/        ドメインモデル (Task / Contract / Project / User / MetadataField)
│   ├── task.rs
│   ├── contract.rs
│   ├── project.rs
│   ├── user.rs
│   └── metadata_field.rs
│
├── application/   ユースケース = ドメインの手続き的実行 + 権限制御
│   ├── task_service.rs
│   ├── contract_service.rs
│   ├── project_service.rs
│   ├── user_service.rs
│   ├── hook_trigger.rs
│   ├── auth.rs
│   └── port/      domain 以外の trait (hook executor, PR verifier 等)
│
├── infra/         port の実装
│   ├── sqlite/
│   ├── postgres/
│   ├── http/      remote backend (= CLI→server HTTP クライアント)
│   ├── hook/      shell hook executor
│   ├── auth.rs    API key / JWT / trusted headers
│   ├── pr_verifier.rs
│   └── config/
│
└── presentation/  入り口
    ├── cli/       clap サブコマンド + handler
    ├── api/       axum ハンドラ (REST API)
    ├── web.rs     読み取り専用 Web ビューア (HTML レンダリング)
    └── dto.rs     presentation ⇄ application 間の DTO
```

## 層ごとの責務

### presentation 層

- **cli**: サブコマンド定義、引数/環境変数/設定ファイルのパース (引数→env→config→default の順でフォールバック)
- **api**: Axum ハンドラ。application service に委譲するだけ
- **web**: HTML レンダリング (読み取り専用)
- **output format**: `--output json|text` の切り替えもここ

### application 層

- **権限制御** (project member / role ベース)
- **ドメインの手続き的実行** = 複数の domain 操作をトランザクショナルに組み合わせる
- **logger / hook executor / PR verifier** 等、業務との結びつきが薄い port をここで定義
- **remote / local の切り替え** — `LocalTaskOperations` と `RemoteTaskOperations` が同じ port を実装

### domain 層

- **aggregate / entity / value object / domain service**
- **repository trait** など、業務との結びつきが深い port はここで定義
- **状態遷移ロジック** (status transition / dependency check / DoD validation)
- aggregate に属する entity の操作は aggregate root を通じて行う

### infra 層

- **port の実装**: SQLite / PostgreSQL repository、HTTP client、shell hook executor、JWT verifier
- **外部サービスドライバ**: AWS Secrets Manager、GitHub CLI (PR verify)
- domain に対する **inbound dependency** なので、方向性を間違えないこと

## port / adapter 対応表

| port (trait) | 定義層 | 実装 (adapter) |
|---|---|---|
| `TaskOperations` | application | `LocalTaskOperations` (→ repository) / `RemoteTaskOperations` (→ HTTP) |
| `ContractOperations` | application | `LocalContractOperations` / `RemoteContractOperations` |
| `ProjectOperations` / `UserOperations` / `MetadataFieldOperations` | application | 同上 |
| `TaskBackend` (repository aggregation) | application | `SqliteBackend` / `PostgresBackend` |
| `HookExecutor` | application | `ShellHookExecutor` |
| `HookDataSource` | application | `SqliteBackend` / `RemoteHookDataSource` |
| `PrVerifier` | application | `GhCliPrVerifier` |
| `AuthProvider` | application | `ApiKeyProvider` / `JwtAuthProvider` / `TrustedHeadersAuthProvider` |

## bootstrap.rs の役割

`src/bootstrap.rs` が **依存関係グラフの組み立て** を一手に引き受けます:

1. `resolve_project_root()` でプロジェクトルート特定
2. `load_config()` で config を読んで runtime に応じた section を有効化
3. `create_backend()` で SQLite or PostgreSQL backend を生成
4. `create_task_operations()` でローカル or HTTP 版の operations を生成
5. 必要なら `HookExecutor` や `AuthProvider` を注入
6. presentation 層 (CLI / API / Web) に `Arc<dyn ...>` として渡す

presentation 層からは `crate::bootstrap::create_task_operations` のように常に bootstrap 経由で依存を取得し、**infra を直接 import しない** ルールです。

## runtime × backend のマトリクス

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

## 非依存ルール

以下は arch-review の対象です:

- domain 層から application / infra / presentation を import してはいけない
- application 層から infra を import してはいけない (bootstrap.rs 以外)
- presentation 層は application のみを呼ぶ。infra / domain を直接 import しない
- port は application か domain に置き、**絶対に infra に置かない**

## 参考

- [docs/knowledge/layered-architecture-design.md](../../knowledge/layered-architecture-design.md) (設計判断の原文)
- arch-review skill (`/arch-review`) で自動チェック可能
