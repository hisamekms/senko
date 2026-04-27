# OTel Tracing 運用ガイド

CLI → Remote の任意属性伝搬と、Remote / Relay 側 OTel SDK での traces / 業務イベント LogRecord 送出を、実際のシェルで動かすためのガイド。

仕様の全量 (33 種の `event_name`、共通属性スキーマ、置換マッピング、baggage 上限) は [Tracing リファレンス](../reference/tracing.md) を参照。

## Claude Code との共存

Claude Code は既に `OTEL_RESOURCE_ATTRIBUTES` / `OTEL_EXPORTER_OTLP_ENDPOINT` などの **OTel 標準環境変数** を使って telemetry を emit しています。senko も同じ変数を読むので、シェルで一度 export するだけで両方に効きます。

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
export OTEL_RESOURCE_ATTRIBUTES="deployment.environment=dev,team=backend"

# senko 側で追加したい動的属性は SENKO_TRACE_ATTRIBUTES か --attr で
export SENKO_TRACE_ATTRIBUTES="run.id=$(uuidgen),session.id=$SESSION_ID"

# Claude Code も senko も同じ collector に送られ、同じ resource 属性が付く
claude ...
senko task complete 42
```

動くこと:

- `deployment.environment` / `team` など **予約 namespace でないキー** は `OTEL_RESOURCE_ATTRIBUTES` から senko が拾って baggage に乗せる → Remote 側で span 属性化される (`baggage.team = "backend"`) と同時に、業務イベント LogRecord にも `team = "backend"` (プレフィックス無し) として attach される。
- `service.name` など **予約 namespace のキー** は senko の baggage には乗らない (大文字混じりも除外)。Claude Code / senko それぞれの OTel SDK が **Resource 属性として** 直接読むため、別ルートで同じ値がバックエンドに届く。

## ローカル検証: console exporter で中身を見る

collector を立てる前に、まず SDK が動くことを目視で確認したい時:

```bash
OTEL_TRACES_EXPORTER=console \
OTEL_LOGS_EXPORTER=console \
senko serve
```

stdout に span / log が JSON で落ちます。別ターミナルから:

```bash
senko --attr run.id=demo1 task complete 42
```

を実行し、サーバ側 stdout に次の 2 系統が出ているのを目視確認します:

- **Span 側**: `attributes` に `baggage.run.id = "demo1"` (`baggage.` プレフィックス付き)
- **LogRecord 側**: `event_name = "senko.task.completed"` の record に、`attributes` として `senko.task.id = 42` / `from_status = "in_progress"` / `to_status = "completed"` / `enduser.id = "<your-user>"` / `senko.operation.id = "<UUID>"` / `run.id = "demo1"` (プレフィックス無し)

業務イベント LogRecord だけを抜き出したい時は `RUST_LOG` を絞ります:

```bash
RUST_LOG=senko_business=info \
OTEL_LOGS_EXPORTER=console \
senko serve
```

infra 系 (`Listening on …`, `OTel telemetry initialized`) は出なくなり、`event_name = "senko.*"` のレコードだけが流れます。

## Jaeger で可視化する

Jaeger の all-in-one コンテナを起動:

```bash
docker run -d --rm \
  -p 16686:16686 \
  -p 4317:4317 \
  jaegertracing/jaeger:latest
```

Remote を OTLP で起動:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
OTEL_SERVICE_NAME=senko-dev \
senko serve
```

別ターミナルから CLI を叩く:

```bash
senko --attr run.id=demo1 --attr user.slot=alice task list
```

`http://localhost:16686` を開き、`Service = senko-dev` で検索。span の attribute に `baggage.run.id=demo1` / `baggage.user.slot=alice` / `http.route=/api/projects/{project_id}/tasks` 等が出ます。

Jaeger の log 検索 (Trace 詳細画面の Logs タブ) では業務イベント LogRecord も同じ trace_id で結合表示されます。`event_name=senko.task.published` で絞り込むと、その trace 上の publish 操作だけが拾えます。

## Tempo で可視化する

Tempo も OTLP gRPC を受けるので、エンドポイント URL を差し替えるだけで同じ手順が使えます。Grafana から Tempo datasource を繋ぎ、TraceQL で:

```traceql
{ event_name = "senko.task.completed" && enduser.id = "alice" }
```

のように業務イベント LogRecord を狙い撃ちできます。`baggage.run.id` (span 属性) と `run.id` (LogRecord 属性) は別フィールドなので、両方クエリしたい時はフィールド名に注意してください。

## Aviary 等の外部システム連携

senko を Aviary (タスクオーケストレータ) など外部システムから呼ぶとき、外部側の相関 ID を `--attr` で渡しておくと **業務イベント LogRecord に自動 attach** されます。これが Contract #8 の主目的のひとつです。

```bash
senko \
  --attr aviary.session.id=sess-abc \
  --attr aviary.nest.id=nest-42 \
  --attr aviary.task.id=at-99 \
  task complete 42
```

→ Remote 側の `senko.task.completed` LogRecord に以下が乗ります:

| カテゴリ | 属性 |
|---|---|
| 業務 (target) | `senko.task.id=42`, `senko.project.id=…`, `from_status=in_progress`, `to_status=completed` |
| actor | `enduser.id=<resolved>`, `enduser.name=<resolved>` |
| 共通 (caller-supplied) | `senko.operation.id=<UUID>`, `aviary.session.id=sess-abc`, `aviary.nest.id=nest-42`, `aviary.task.id=at-99` |
| Resource | `service.name=senko-server` (Remote) / `senko-relay` (Relay), `service.version=…`, `senko.version=…` |

Aviary 側のオブザーバビリティ基盤 (Loki / Datadog / Splunk 等) では `aviary.session.id="sess-abc"` で絞り込むと、その session 内の senko 操作 (複数 task complete / status 変更等) を時系列に並べられます。

### Relay 経由でも同じ属性が付く

`senko serve --proxy` (Relay) を経由する場合も同じです:

```text
[CLI]
  └── --attr aviary.session.id=sess-abc
       │
       ▼ HTTP (baggage ヘッダ)
[Relay (senko serve --proxy)]
  ├── 自身の senko.api.call にも aviary.session.id=sess-abc が乗る
  ├── 上流 /auth/me を LRU(5min) で resolve → enduser.* も乗る
  └── 上流に baggage を逐語転送
       │
       ▼
[Remote (senko serve)]
  └── senko.task.completed LogRecord に aviary.session.id=sess-abc + enduser.* + 全 Resource 属性
```

Relay 自身の telemetry にも `enduser.*` が乗るので、Aviary 連携時に「Relay は通ったが Remote には届かなかった」のような中間障害も同じ session ID で追跡できます。

### `senko.version` でバイナリの provenance を確認

`senko.version` は Resource 属性として **常に**、env からの上書き不可で出ます。混在環境 (Remote バージョン違い、Relay と Remote のバージョン乖離) では `senko.version` で送信元バイナリを特定できます。

## 監査・操作追跡

Contract #8 で `enduser.*` (actor) と `senko.*.id` (target) を分離したので、業務イベント LogRecord は次のように利用できます。

### 「誰がやったか」を追う (actor 軸)

```traceql
{ event_name =~ "senko\\..*" && enduser.id = "alice" }
```

`alice` が走らせた全操作 (task complete / project edit / API key 発行等) を時系列で並べる。`enduser.id` は OTel 標準の semantic convention なので Loki / Tempo / Datadog のいずれでも同じフィールド名で引けます。

### 「対象が誰だったか」を追う (target 軸)

別 user (`bob`) のセッションが取り消された / role を変えられた等の追跡:

```traceql
{ event_name = "senko.user.session_revoked" && senko.user.id = 7 }
{ event_name = "senko.project.member_role_changed" && senko.user.id = 7 }
```

`senko.user.id` は senko 独自の **target** 識別子です。`enduser.id` (= 操作した人の username) と混同しないでください。両方が出る LogRecord は「`alice` が `bob` のセッションを取り消した」のように、actor と target の両方を保持します。

### 1 CLI invocation 内の全 LogRecord / span を辿る

`senko --attr foo=bar task add ... && senko ... && senko ...` のように複数コマンドを 1 つの操作として束ねたい時は、上位スクリプトで `SENKO_TRACE_ATTRIBUTES=senko.operation.id=<own-id>` を export してから呼ぶことで、全 invocation に同じ correlation ID が乗ります。

```traceql
{ senko.operation.id = "abc-123" }
```

で span / LogRecord 両方の発火をすべて拾えます。

## 検証チェックリスト

実装が期待通りに動いているか確認する最小項目:

- [ ] Remote 側 `console` exporter / log backend で `event_name = "senko.*"` の LogRecord が出る (例: `senko task complete N` 後に `senko.task.completed`)
- [ ] 各 LogRecord に `enduser.id` / `enduser.name` (auth 認証下) と `senko.operation.id` が乗っている
- [ ] Resource 属性 `service.name` (Remote: `senko-server` / Relay: `senko-relay`、env で上書き可) / `service.version` / `senko.version` がすべての record に付く
- [ ] `--attr aviary.session.id=foo` した時、`senko.task.completed` の attributes に `aviary.session.id=foo` (プレフィックス無し) が乗る
- [ ] Span 側にも `baggage.run.id=demo1` が付いている (別経路、同 trace_id)
- [ ] `OTEL_RESOURCE_ATTRIBUTES` に `service.name=foo` を入れても baggage 経由で `service.name` が **流れない** こと (大文字混じり `SERVICE.NAME` も同様に除外)
- [ ] `--attr` と `OTEL_RESOURCE_ATTRIBUTES` に同じキーを入れると **`--attr` の値が勝つ** こと
- [ ] `OTEL_SDK_DISABLED=true` 下では console / OTLP どちらの exporter も無音になること (fmt ログは残る)
- [ ] baggage 1 値 256 byte 超で warn + 切り詰め
- [ ] baggage キー 32 個超で warn + アルファベット末尾 drop
- [ ] baggage 合計 8 KB 超で warn + 末尾 drop
- [ ] `senko.api.call` の `http.route` がテンプレート (`/api/projects/{project_id}/tasks`) で、生 URL や query を含まない
- [ ] `senko.api.error` で `error.type=internal` の `error.message` に Display フォーマットされた anyhow chain が出る (Debug `?e` 形式ではない)
- [ ] hook 起動成功で `senko.hook.fired`、失敗で `senko.hook.failed`(`failure.reason`) が出る (sync mode で `enduser.*` が乗ること)

## 無効化したいとき

完全に止めたい時:

```bash
OTEL_SDK_DISABLED=true senko serve
```

これで collector への発呼も console 出力もゼロになります (fmt ログは残る)。部分的に止めたい時は exporter 側で:

```bash
OTEL_TRACES_EXPORTER=none OTEL_LOGS_EXPORTER=none senko serve
```

`OTEL_LOGS_EXPORTER=none` を指定すると業務イベント LogRecord も OTLP に流れなくなります (fmt layer / stdout には残る)。

## セキュリティ考慮

- **認可には使わない**。 baggage / resource attrs / `enduser.*` は CLI 側で自由に書けます (`--attr`・任意の env)。Remote 側の権限判定 (user id の推定、role の解釈など) に使ってはいけません。**`enduser.*` は auth middleware が認証済 identity から resolve した値**なので business observability 用途には信頼できますが、これも認可ロジックの直接入力にはせず、必ず OIDC / API key / 信頼ヘッダなど **認証済み identity** 経由で行ってください。
- **PII を載せない**。 baggage は HTTP ヘッダとしてワイヤに乗り、span / 業務イベント LogRecord としてバックエンドに **長期保存** されます。メールアドレス・パスワード・API token・リクエスト本文・path 内の機密 ID などは値にしないこと。`http.route` がテンプレートに変わったのもこの目的の一部です (URL に機密値が乗らない)。
- **値・キー・合計サイズに上限がある**。1 値 256 byte / キー数 32 / 合計 8 KB を超えると Remote 側で切り詰め・drop されます。切り詰め前の値・全キー存在を前提にクエリや集計を組まないこと。
- **不正値はサイレントに消える**。 `OTEL_RESOURCE_ATTRIBUTES` / `SENKO_TRACE_ATTRIBUTES` の malformed エントリは OTel spec に従って **silently skip** されます。設定ミスに気付きたい時は代わりに `--attr` を使ってください (malformed ならエラー終了します)。
- **予約 namespace は大文字小文字無視**。`SERVICE.NAME` 等の大文字バリアントで filter を回避することはできません (Phase F2 で塞ぎ済み)。

## 関連

- 仕様全量: [Tracing リファレンス](../reference/tracing.md)
- Remote のデプロイ: [server-remote デプロイ](server-remote/deploy.md)
