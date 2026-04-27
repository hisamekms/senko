# Tracing リファレンス

senko の Remote / Relay は次の 2 系統で観測性データを発信します。

1. **W3C Trace Context + Baggage**: CLI → Remote のリクエストに任意属性を伝搬し、Remote 側で `baggage.<key>` span attribute に昇格する。
2. **業務イベント LogRecord (各 record の `event_name` フィールドに `senko.*` が入る)**: Remote / Relay のアプリケーション層で、ドメイン状態遷移ごとに OTel `LogRecord` を発火する。Aviary 等の外部システムから `--attr aviary.session.id=…` で渡された baggage が、この LogRecord に **そのまま** 共通属性として乗る。

両系統とも標準 OTel SDK 経由で出るので、Claude Code など他の OTel-aware なツールと **同じ環境変数** で運用できます。運用手順は [OTel Tracing 運用ガイド](../guides/tracing.md) を参照。

> **CLI local mode (sqlite/postgres バックエンド直叩き) は OTel SDK を初期化しません。** 業務イベント / 横断イベントが出るのは Remote (`senko serve`) と Relay (`senko serve --proxy`) のみです。

## 業務イベントの全体像

業務イベントは `tracing::event!` を `target: "senko_business"` 固定で発火し、`opentelemetry-appender-tracing` の `OpenTelemetryTracingBridge` が `Metadata::name()` を OTel `LogRecord::set_event_name` に転写します。

> **重要**: `event_name` は OTLP `LogRecord` proto の **トップレベルフィールド** (field 12) であり、`attributes` 配列には入りません。下流コンシューマは LogRecord オブジェクトの `event_name` (snake_case) フィールドを直接参照してください。attributes 側を見ても何も得られません。詳細は [下流コンシューマでの `event_name` 参照](#下流コンシューマでの-event_name-参照) を参照。

| 項目 | 値 |
|---|---|
| `target` | `senko_business` (固定) |
| `Level` | `INFO` (通常) / `WARN` (`senko.hook.failed`) / `ERROR` (`senko.api.error`) |
| 出力先 | fmt layer (stdout JSON) と OTel Logs exporter の **両方** に同じレコードが流れる |
| 共通属性 | Resource / actor / target / `senko.operation.id` を SDK と `BusinessAttributesProcessor` が自動 attach |
| emit 層 | アプリケーション層 (`LocalXxxOperations` / `RemoteXxxOperations` / `XxxService` 等) と一部 middleware (`presentation/api/telemetry.rs` / `infra/hook/mod.rs`) |
| `RUST_LOG` フィルタ | `RUST_LOG=senko_business=info` で業務イベントだけを取り出せる |

`BusinessAttributesProcessor` は OTel の `LogProcessor` として登録され、`target == "senko_business"` のレコードに対してだけ tokio task-local 由来の属性を attach します:

- `RESOLVED_USER` (auth middleware が injection): → `enduser.id` / `enduser.name`
- `INBOUND_BAGGAGE` (`propagate_trace_context` middleware が injection): → `senko.operation.id` および任意の caller-supplied 属性 (`aviary.session.id` 等)。**baggage. プレフィックスは付かず、原 key 名のまま**

infra 系 (`info!("Listening on …")` 等) は `target` がモジュールパスのままなので Processor は素通り、Resource 属性のみが付きます。

## event_name 一覧 (合計 33 種)

29 種の業務イベントと 4 種の横断イベントの合計 33 種を発火します。Aviary 等の外部システムから渡された `--attr aviary.*=…` の baggage は、これら **すべて** に共通属性として乗ります。

### Task (11 種)

| `event_name` | タイミング | 必須属性 (共通属性に加えて) |
|---|---|---|
| `senko.task.created` | `task add` 成功時 | `senko.task.id`, `senko.project.id` |
| `senko.task.updated` | `task edit` 成功時 (title / description / priority / plan / tags / metadata 等) | `senko.task.id`, `senko.project.id`, `changed_fields` (JSON 配列) |
| `senko.task.published` | `task publish` 成功時 | `senko.task.id`, `senko.project.id`, `from_status`, `to_status` |
| `senko.task.started` | `task start` 成功時 | `senko.task.id`, `senko.project.id`, `from_status`, `to_status` |
| `senko.task.completed` | `task complete` 成功時 | `senko.task.id`, `senko.project.id`, `from_status`, `to_status` |
| `senko.task.canceled` | `task cancel` 成功時 | `senko.task.id`, `senko.project.id`, `from_status`, `to_status`, `cancel_reason` |
| `senko.task.dependency_added` | `deps add` 成功時 | `senko.task.id`, `senko.project.id`, `dep_id` |
| `senko.task.dependency_removed` | `deps remove` 成功時 | `senko.task.id`, `senko.project.id`, `dep_id` |
| `senko.task.dependencies_set` | `deps set` 成功時 | `senko.task.id`, `senko.project.id`, `deps` (JSON 配列) |
| `senko.task.dod_checked` | `dod check` 成功時 | `senko.task.id`, `senko.project.id`, `dod_index` |
| `senko.task.dod_unchecked` | `dod uncheck` 成功時 | `senko.task.id`, `senko.project.id`, `dod_index` |

### Contract (6 種)

| `event_name` | タイミング | 必須属性 |
|---|---|---|
| `senko.contract.created` | `contract create` 成功時 | `senko.contract.id`, `senko.project.id` |
| `senko.contract.updated` | `contract edit` 成功時 | `senko.contract.id`, `senko.project.id`, `changed_fields` |
| `senko.contract.deleted` | `contract delete` 成功時 | `senko.contract.id`, `senko.project.id` |
| `senko.contract.dod_checked` | `contract dod check` 成功時 | `senko.contract.id`, `senko.project.id`, `dod_index` |
| `senko.contract.dod_unchecked` | `contract dod uncheck` 成功時 | `senko.contract.id`, `senko.project.id`, `dod_index` |
| `senko.contract.note_added` | `contract note add` 成功時 | `senko.contract.id`, `senko.project.id` |

### Project (5 種)

| `event_name` | タイミング | 必須属性 |
|---|---|---|
| `senko.project.created` | `project create` 成功時 | `senko.project.id` |
| `senko.project.updated` | `project edit` 成功時 | `senko.project.id`, `changed_fields` |
| `senko.project.member_added` | member 追加成功時 | `senko.project.id`, `senko.user.id` (target), `role` |
| `senko.project.member_removed` | member 削除成功時 | `senko.project.id`, `senko.user.id` (target) |
| `senko.project.member_role_changed` | role 変更成功時 | `senko.project.id`, `senko.user.id` (target), `from_role`, `to_role` |

### User (5 種)

| `event_name` | タイミング | 必須属性 |
|---|---|---|
| `senko.user.created` | `user create` / 自動プロビジョニング成功時 | `senko.user.id` (target), `source` (`manual` / `oidc_provisioning` / `trusted_headers_provisioning`) |
| `senko.user.updated` | `user edit` 成功時 | `senko.user.id` (target), `changed_fields` |
| `senko.user.api_key_issued` | API key 発行成功時 | `senko.user.id` (target) |
| `senko.user.api_key_revoked` | API key 取消成功時 | `senko.user.id` (target) |
| `senko.user.session_revoked` | session revoke 成功時 (single / all) | `senko.user.id` (target), `session.id`, `scope` (`Single` / `All`) |

`scope=All` の場合は **対象セッションごとに 1 LogRecord** を emit します (まとめ 1 件ではない)。

### MetadataField (2 種)

| `event_name` | タイミング | 必須属性 |
|---|---|---|
| `senko.metadata_field.defined` | `metadata-field define` 成功時 | `senko.project.id`, `senko.metadata_field.name`, `senko.metadata_field.type` |
| `senko.metadata_field.removed` | `metadata-field remove` 成功時 | `senko.project.id`, `senko.metadata_field.name`, `senko.metadata_field.type` (削除前の値) |

### 横断イベント (4 種)

middleware / 横断レイヤで emit。業務イベントと違いドメイン集約に紐付かない。

| `event_name` | タイミング | emit 場所 | 必須属性 |
|---|---|---|---|
| `senko.api.call` | リクエスト終了時 (200 / 5xx いずれも 1 件) | `propagate_trace_context` middleware | `http.method`, `http.route`, `http.status_code`, `latency_ms`, [`senko.project.id`] |
| `senko.api.error` | `ApiError` レスポンス時 | `IntoResponse for ApiError` | `http.status_code`, `error.type`, `error.message` |
| `senko.hook.fired` | hook 起動成功時 (exit 0) | `ShellHookExecutor` | `hook.name`, `hook.trigger`, `exit_status=0`, `duration_ms` |
| `senko.hook.failed` | hook 失敗時 (`timeout` / `spawn_error` / `non_zero_exit` / `stdin_error` / `wait_error`) | `ShellHookExecutor` | `hook.name`, `hook.trigger`, `failure.reason`, `duration_ms`, [`exit_status`], [`stderr_excerpt`], [`error.message`] |

`http.route` は **axum の `MatchedPath` (テンプレート文字列)** で、URL のクエリ文字列は含まれません (例: `/api/projects/{project_id}/tasks`)。マッチしない 404 は `uri.path()` (実パス、クエリ無し) に fallback します。

`error.type` は `not_found` / `bad_request` / `unauthorized` / `forbidden` / `conflict` / `internal` / `not_implemented` のいずれか。`error.message` は `ApiError::Internal` の場合のみ public response とは別の `log_message` (Display フォーマット済 anyhow chain) が乗り、レスポンス本文には常に `"internal server error"` という固定文字列が返ります (内部情報が漏出しない)。

`hook.trigger` は hook 設定キー (`task_complete` / `task_update` / `project_member_added` / `user_api_key_issued` 等)。`stderr_excerpt` は最大 1024 byte (UTF-8 lossy)。

> **既知ギャップ (Phase E1/V1 で要対応)**: hook の `mode = async` 経路は `std::thread::spawn` の worker 内で emit するため、tokio task-local (`RESOLVED_USER` / `INBOUND_BAGGAGE`) が伝播せず、`senko.hook.fired` / `senko.hook.failed` の `enduser.*` / `senko.operation.id` は付きません。`mode = sync` 経路は付きます。

## 共通属性スキーマ

業務イベント (`target=senko_business`) に **すべて** 自動 attach される属性の一覧です。横断イベントも同じ機構の上に乗っています。

### Resource 属性 (起動時固定、SDK が直接 attach)

| 属性 | 既定値 | env 上書き |
|---|---|---|
| `service.name` | `senko-server` (Remote) / `senko-relay` (Relay) | `OTEL_SERVICE_NAME` で上書き可。`OTEL_RESOURCE_ATTRIBUTES=service.name=…` でも可 |
| `service.version` | ビルド時の `CARGO_PKG_VERSION` | `OTEL_RESOURCE_ATTRIBUTES=service.version=…` で上書き可 |
| `senko.version` | ビルド時の `CARGO_PKG_VERSION` | **上書き不可**。telemetry データの provenance 担保のため senko バイナリが常に自己申告 |

`senko.version` は Aviary 連携仕様で必須とされているため、env による書き換えはできません。「どのバージョンの senko が emit したか」の真実を保つための設計判断です。

### actor — 操作した人 (OTel semconv)

| 属性 | 値 |
|---|---|
| `enduser.id` | 認証済み user の `username` (Remote の auth middleware が `RESOLVED_USER` task-local 経由で attach) |
| `enduser.name` | 同 user の表示名 (`display_name` または `username`) |

未認証 (`/healthz` 等のパブリックエンドポイント) では attach されません。

### target — 操作の対象 (senko 独自)

ドメインごとに対応する識別子。1 LogRecord に actor と target の両方が付くのは「自分以外を操作した」ケース (`senko.project.member_added` で別 user を追加、`senko.user.session_revoked` で別 user の session を取消、等)。

| 属性 | 載るイベント |
|---|---|
| `senko.task.id` | `senko.task.*` 全種 |
| `senko.contract.id` | `senko.contract.*` 全種 |
| `senko.project.id` | `senko.{task,contract,project,metadata_field}.*` 全種、および `senko.api.call` (path に `{project_id}` を含む場合) |
| `senko.user.id` | `senko.user.*` 全種 (= target user の id)、`senko.project.member_*` (= 追加 / 削除 / role 変更された user の id) |
| `senko.metadata_field.name` / `senko.metadata_field.type` | `senko.metadata_field.*` 全種 |

### 共通 (caller-supplied baggage 由来、`BusinessAttributesProcessor` が attach)

| 属性 | 値 |
|---|---|
| `senko.operation.id` | CLI プロセスで採番された UUIDv4 (1 操作内の全 LogRecord / span を相関づける) |
| 任意 caller-supplied 属性 | CLI 側で `--attr foo=bar` した baggage がそのまま (例: `aviary.session.id`, `aviary.nest.id`, `aviary.task.id`)。**`baggage.` プレフィックスは付かず原 key 名のまま** |

caller-supplied 属性は予約 namespace フィルタを通った後の最終形が乗ります ([予約 namespace](#予約-namespace) 参照)。

### HTTP 属性 (`senko.api.call` / `senko.api.error` のみ)

| 属性 | 値 |
|---|---|
| `http.method` | リクエストメソッド (`GET` / `POST` 等) |
| `http.route` | axum の `MatchedPath` テンプレート (例: `/api/projects/{project_id}/tasks/{id}`)、クエリ文字列なし |
| `http.status_code` | レスポンスの整数ステータスコード |
| `latency_ms` | リクエスト処理時間 (整数 ms) |

> **重要**: 旧来の `http.target` (実 URL = path + query) は **削除** され、`http.route` (テンプレート) に置き換わりました。これにより query string や path 内の機密値 (`/api/users/{user_id}/api-keys/{key_id}` 等) が telemetry に永続化されません。

## 既存 tracing → 新 event 置換マッピング

Contract #8 で以下の bare tracing は新業務イベントに置換され、二重出力しません。

| 旧出力 | 新 event | 備考 |
|---|---|---|
| `tracing::warn!("api_error", …)` (`presentation/api/mod.rs`) | `senko.api.error` | `error.type` / `error.message` を構造化 |
| `tracing::error!("unclassified internal error", ?e)` | `senko.api.error` (`error.type=internal`) | `?e` (Debug-format) → `%e` (Display)。anyhow chain は `format!("{e:#}")` で flatten |
| `tracing::info!("auto-provisioning user from OIDC claims")` (`infra/auth.rs`) | `senko.user.created` (`source=oidc_provisioning`) | — |
| `tracing::info!("auto-provisioning user from trusted headers")` | `senko.user.created` (`source=trusted_headers_provisioning`) | — |
| `tracing::info!("response", …)` / `error!("request failed", …)` (`presentation/api/telemetry.rs`) | `senko.api.call` (200/5xx 1 件) / `senko.api.error` | — |
| Hook 系 `tracing::warn!` (`infra/hook/mod.rs`) | `senko.hook.failed` | `failure.reason` / `stderr_excerpt` 構造化 |

## 維持される bare tracing (置換しない)

「インフラのライフサイクル」「設定 / 起動時バリデーション」「ネットワーク失敗」等の業務イベントに分類できないものは、bare `tracing::info!` / `warn!` のまま残します。

- `info!("Listening on {addr}")` (起動時)
- `info!("OTel telemetry initialized")` / `info!("OTel telemetry disabled (OTEL_SDK_DISABLED=true)")` (bootstrap)
- `info!("shutdown signal received")` (graceful shutdown)
- `warn!("baggage value truncated", …)` / `warn!("baggage drops excess key", …)` / `warn!("baggage total size exceeded", …)` (受信側 sanitization)
- `warn!("OIDC discovery failed")` / `warn!("JWKS fetch failed")` (auth bootstrap の transient 失敗)
- `validate_hook_def` / `warn_about_mismatched_runtime_sections` の起動時 warn (config 妥当性)
- 各種 `tracing::debug!` (auth claim mismatch 等)

これらは business observability ではなく operations observability の対象です。

## CLI が送る HTTP ヘッダ

CLI → Remote の各リクエストに次のヘッダが付与されます。

| ヘッダ | 付与条件 | 中身 |
|---|---|---|
| `traceparent` | **毎回 (常時)** | `version-trace_id-parent_id-flags` (W3C Trace Context)。`trace_id` 128-bit + `span_id` 64-bit をリクエストごとに新規生成 |
| `baggage` | マージ後の属性マップが **非空の時のみ** | `key1=value1,key2=value2` (W3C Baggage)。キー・値ともに percent-encoded (`NON_ALPHANUMERIC`) なので `.` `=` `,` 空白はすべて `%..` エンコードされる |

属性を一つも指定しない時は `baggage` は付かず、`traceparent` だけが流れます。

## 属性の 4 ソースと優先順位

CLI が baggage に載せる属性は、次の 4 ソースをマージして決まります。

| ソース | 形式 | malformed の扱い | 予約 namespace フィルタ |
|---|---|---|---|
| `--attr KEY=VALUE` (CLI グローバルフラグ、繰り返し可) | 1 回 1 ペア | **エラー終了** (`invalid --attr …`) | 適用しない |
| `SENKO_TRACE_ATTRIBUTES` (環境変数) | `K=V,K=V,…` | silent skip (OTel spec 準拠) | 適用しない |
| `OTEL_RESOURCE_ATTRIBUTES` (環境変数) | `K=V,K=V,…` | silent skip | **適用する** |
| 自動採番 (`senko.operation.id`) | CLI プロセスで UUIDv4 を 1 回だけ採番 | — | 適用しない (内部生成値) |

### 優先順位

同じキーが複数ソースに現れた時、**`--attr` > `SENKO_TRACE_ATTRIBUTES` > `OTEL_RESOURCE_ATTRIBUTES` > 自動採番** の順で高優先側が勝ちます。自動採番の値は上 3 ソースのどれでも上書きできます。

### 自動採番される属性

| キー | 値 | 採番タイミング |
|---|---|---|
| `senko.operation.id` | UUIDv4 文字列 | CLI プロセス起動後、最初に trace 属性が解決されるタイミングで 1 回だけ採番し、同一プロセス内で再利用 |

`senko.operation.id` は「1 CLI invocation 内の複数 HTTP リクエスト / hook / status 変更」を Remote 側で相関づけるための相関 ID です。同一 `senko …` コマンド内の全ての baggage に同じ値が乗るので、Remote 側で `senko.operation.id` を絞り込みキーにすると 1 操作に紐付く全 span / log を一覧できます。ユーザーが `--attr senko.operation.id=<own-id>` や `SENKO_TRACE_ATTRIBUTES=senko.operation.id=<own-id>` で上書きした場合はその値が勝ちます。

### `--attr` の使い方

`--attr` は **グローバルフラグ** です。サブコマンドより前に置いてください。

```bash
senko --attr run.id=abc123 --attr session.id=xyz task complete 42
```

malformed な値は途中で落とさずエラー終了します (OTel spec の env 変数と異なる点):

| 入力 | エラー文言 |
|---|---|
| `--attr foo` (`=` なし) | `invalid --attr 'foo': expected KEY=VALUE` |
| `--attr =bar` (キー空) | `invalid --attr '=bar': key must not be empty` |
| `--attr foo=` (値空) | `invalid --attr 'foo=': value must not be empty` |

#### Aviary 等の caller-supplied 属性

外部システム連携では、システム固有の相関 ID を `--attr` で複数渡します。これらは **業務イベント LogRecord にそのまま attach** されます (baggage. プレフィックス無し)。

```bash
senko \
  --attr aviary.session.id=sess-abc \
  --attr aviary.nest.id=nest-42 \
  --attr aviary.task.id=at-99 \
  task complete 42
```

→ Remote では `senko.task.completed` LogRecord に以下が attach されます:

- 業務側: `senko.task.id=42`, `senko.project.id=…`, `from_status=in_progress`, `to_status=completed`
- actor: `enduser.id=…`, `enduser.name=…`
- 共通: `senko.operation.id=<UUID>`, `aviary.session.id=sess-abc`, `aviary.nest.id=nest-42`, `aviary.task.id=at-99`
- Resource: `service.name=senko-server`, `service.version=…`, `senko.version=…`

Aviary 側は `aviary.session.id` で同 session 内の全 senko 操作を Jaeger / Tempo / Logging backend で集計できます。

### `SENKO_TRACE_ATTRIBUTES` / `OTEL_RESOURCE_ATTRIBUTES` のパース挙動

- `K=V` 形式を `,` で区切る (OTel Resource Attributes 仕様と同じ)
- キー周辺の空白はトリムされる
- **値の空白は保持される** (`foo= bar` → 値は ` bar`)
- malformed エントリ (`=` なし / キー空 / 値空) は **silently skip** する (ログも出ない)
- 空文字列の場合は属性 0 件として扱う

## 予約 namespace

OTel が定義する Resource 属性との衝突を避けるため、`OTEL_RESOURCE_ATTRIBUTES` からは次の prefix のキーを **自動除外** します。**判定は大文字小文字無視 (case-insensitive)**:

```
service.  host.  os.  process.  telemetry.  deployment.  cloud.  k8s.  container.
```

`SERVICE.NAME` / `Service.Name` / `service.name` のいずれもフィルタされます (大文字混じりの bypass を防ぐため、Contract #8 / F2 で導入)。

この除外は **`OTEL_RESOURCE_ATTRIBUTES` のみ** に適用されます。ユーザが明示的に書いた `--attr` や `SENKO_TRACE_ATTRIBUTES` の値はフィルタされません (明示指定はユーザ意図を尊重する)。受信側 (Remote の `propagate_trace_context`) でも防御的に同じフィルタを再適用します (二重防御)。

## baggage 上限 (受信側)

Remote / Relay が受信した baggage は、span attribute 昇格 / span 後段 emit に渡される前に **1 度だけ正規化** されます (`apply_baggage_limits`)。Relay モードでは正規化済の値がそのまま上流へ forward されるので、Relay が DoS 増幅器にならない設計です。

| 制限 | 値 | 超過時の挙動 |
|---|---|---|
| 1 値あたりの長さ | **256 byte** (UTF-8 境界で切り詰め) | `tracing::warn!("baggage value truncated", …)` を出して切り詰め後の値を採用 |
| キー数 | **32 キー** | `tracing::warn!("baggage drops excess key", …)` を出して **アルファベット順で 32 番目以降を drop** (head retain) |
| 合計 byte 数 | **8 KB (8 × 1024 byte)** | `tracing::warn!("baggage total size exceeded", …)` を出して **末尾から (アルファベット降順) drop**、合計が 8 KB 以下になるまで繰り返し |

正規化順序は (1) キー数上限 → 32 head retain、(2) 値長 → 256 B 切詰、(3) 合計長 → 末尾から drop の固定順。CLI 側では切り詰めしません (受信側責務に統一)。

## Remote (`senko serve`) が読む OTel 環境変数

Remote は起動時に次の標準 OTel 環境変数を読み、OTel SDK を初期化します。

| 変数 | 値 | 既定 | 挙動 |
|---|---|---|---|
| `OTEL_SDK_DISABLED` | `true` / `false` | `false` | `true` で OTel SDK 全層を無効化 (fmt ログ + W3C propagator のみ残る) |
| `OTEL_TRACES_EXPORTER` | `otlp` / `console` / `none` | `none` (※) | traces の送出先 |
| `OTEL_LOGS_EXPORTER` | `otlp` / `console` / `none` | `none` (※) | logs の送出先 (業務イベント LogRecord はここを通る) |
| `OTEL_SERVICE_NAME` | 文字列 | `senko-server` (Remote) / `senko-relay` (Relay) | `service.name` Resource 属性の値 |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | URL | — | OTLP collector。これが設定されると exporter 既定が `otlp` に昇格 |
| `OTEL_RESOURCE_ATTRIBUTES` | `K=V,K=V,…` | — | OTel SDK の Resource 属性 (SDK が直接読む)。`service.name=…` / `service.version=…` を含めると senko の既定値 (それぞれ mode 別の `senko-server` / `senko-relay`、ビルド時の `CARGO_PKG_VERSION`) を上書きできる。`service.name` については `OTEL_SERVICE_NAME` のほうが優先される (OTel SDK 標準) |

> **(※) 既定値についての注意**: OTel 仕様の既定は `otlp` ですが、senko Remote は **OTel env が何も設定されていない時は `none`** にしています (ローカル開発で意図せぬ OTLP 接続を避けるため)。`OTEL_EXPORTER_OTLP_ENDPOINT` を設定すると exporter の既定が `otlp` に昇格します。未知の exporter 名は警告ログを出して `none` として扱います。

例: `OTEL_SERVICE_NAME=senko-prod senko serve` で起動すると、Resource 属性 `service.name=senko-prod` で OTLP に export されます (Remote / Relay どちらの mode でも同様)。

## Baggage → Span attribute の昇格

Remote 側のミドルウェア (`presentation/api/telemetry.rs` の `propagate_trace_context`) が、受信 `baggage` ヘッダの各エントリを **`baggage.<key>`** という名前の span attribute として span に付与します。

- これは `tracing::Span` への `record` 経由で、**業務イベント LogRecord (`target=senko_business`) とは別経路** です。span は OTLP traces 側に流れ、業務イベントは OTLP logs 側に流れます (両方に同じ trace_id が付くので backend 上で結合可能)。
- 予約 namespace のキーは防御的に除外されます ([予約 namespace](#予約-namespace) 参照)
- 値は 256 byte で UTF-8 境界切り詰め
- 切り詰めが起きた時は `tracing::warn!` で警告

これにより、baggage で届いた `run.id=xyz` 等は Jaeger / Tempo 等のバックエンドで `baggage.run.id = "xyz"` として検索可能になります。一方で **業務イベント LogRecord** の側では `run.id=xyz` (プレフィックス無し) として attach されるので、log filter は原 key 名で書きます。

## Proxy モードの注意

`senko serve --proxy` は、上流 Remote への転送時に **新しい `traceparent`** を発行します (パススルーではなく再発射)。インバウンドの `baggage` は抽出されて上流への転送リクエストにも再発射されるため、CLI が発した `baggage.<key>` は上流 Remote のスパンにそのまま現れます。

- Relay 側では予約 namespace の再フィルタを行いません (CLI 側で既に整形済。二重フィルタは `--attr` 等で明示指定されたキーを意図せず落とすことになるため)
- 上流 Remote の `propagate_trace_context` が受信した `baggage` を `baggage.<key>` span attribute に昇格する際、防御的に予約 namespace を除外するのは従来通り

### Relay 自身の業務イベントへの enduser 注入

Relay は **自身のテレメトリ** (Relay で emit する `senko.api.call` / `senko.api.error` / `senko.hook.*` 等) にも `enduser.*` を載せたいので、上流 Remote の `/auth/me` を **LRU キャッシュ + 5 分 TTL** で叩いて principal を resolve し、`RESOLVED_USER` task-local に inject します。

cache key は次の 3 段で算出 (上から優先):

1. Bearer JWT の `sub` claim を抽出 → `jwt:<sub>` (署名検証はせず手動 base64 decode で `sub` だけ取る)
2. JWT 形式でない / `sub` が無い不透明 token → `tok:<sha256_hex_of_token>`
3. 信頼ヘッダモードの `subject_header` 値 → `thv:<value>`

fetch 失敗時 (network / non-2xx / parse error / 必須フィールド欠) は cache に入れず、`enduser.*` 不在のまま上流転送 / 業務イベント emit を続行します (graceful degrade)。fetch timeout は 5 秒。

## グレースフルシャットダウン

`SIGINT` (Ctrl-C) と `SIGTERM` を受けると、Remote / Relay は axum の in-flight リクエストを drain したあと、OTel の tracer / logger provider を flush してから終了します。telemetry 系のドロップは **明示的な flush 後** に起きるので、短寿命プロセスでも最後のスパンと業務イベント LogRecord が送出されます。

## 下流コンシューマでの event_name 参照

OTLP wire 上、`event_name` は `LogRecord` proto の **トップレベルフィールド** (field 12) です。`attributes` 配列には入らないため、attribute ベースで集計しているコードは値を取得できません。**`attributes["event.name"]` を見ても何も得られません。**

### バックエンド / SDK 別の参照パス

| バックエンド / SDK | 参照パス |
|---|---|
| OTel Collector pipeline (transform processor 等) | OTTL の `log` context で `log.event_name` |
| `opentelemetry-proto` (Rust / Go / Python 等) | `LogRecord.event_name` (snake_case) フィールドを直読 |
| Grafana Loki (otelcol → loki exporter 経由) | label `event_name` (デフォルト promotion 設定時) |
| Grafana Tempo (Trace 詳細の Logs タブ) | フィールド名 `event_name` |
| 自前の OTLP receiver | proto field 12 を直接デコード |

### 旧 `attributes["event.name"]` 互換が必要な場合

attribute ベースで書かれた既存集計を書き換えずに済ませるには、OTel Collector の `transform` processor で `event_name` を attribute にコピーするのが実用的です:

```yaml
processors:
  transform/event_name_compat:
    log_statements:
      - context: log
        statements:
          - set(attributes["event.name"], event_name) where event_name != nil
```

senko 側は OTel Logs Data Model 新仕様 (`LogRecord.event_name`) に準拠しており、互換のための dual-emit はしません (=「attribute 側にも同じ値を二重で乗せる」ことはしません)。旧仕様前提のコンシューマは Collector レイヤで吸収してください。

## 関連

- [`--attr` グローバルフラグ](cli.md#グローバルオプション)
- [OTel Tracing 運用ガイド](../guides/tracing.md) — Aviary 連携、event_name クエリ、監査用フィルタ、Jaeger / Tempo / console exporter の検証手順、セキュリティ考慮
