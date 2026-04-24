# Tracing リファレンス

senko CLI は **W3C Trace Context + Baggage** で Remote に任意属性を伝搬し、Remote 側は **OpenTelemetry SDK** 経由で traces と logs を emit します。標準仕様ベースなので、Claude Code など他の OTel-aware なツールと **同じ環境変数** だけで運用できます。

運用手順は [OTel Tracing 運用ガイド](../guides/tracing.md) を参照。

## 送信される HTTP ヘッダ

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

`senko.operation.id` は「1 CLI invocation 内の複数 HTTP リクエスト / hook / status 変更」を Remote 側で相関づけるための相関 ID です。同一 `senko …` コマンド内の全ての baggage に同じ値が乗るので、Remote 側で `baggage.senko.operation.id` を絞り込みキーにすると 1 操作に紐付く全 span / log を一覧できます。ユーザーが `--attr senko.operation.id=<own-id>` や `SENKO_TRACE_ATTRIBUTES=senko.operation.id=<own-id>` で上書きした場合はその値が勝ちます。

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

### `SENKO_TRACE_ATTRIBUTES` / `OTEL_RESOURCE_ATTRIBUTES` のパース挙動

- `K=V` 形式を `,` で区切る (OTel Resource Attributes 仕様と同じ)
- キー周辺の空白はトリムされる
- **値の空白は保持される** (`foo= bar` → 値は ` bar`)
- malformed エントリ (`=` なし / キー空 / 値空) は **silently skip** する (ログも出ない)
- 空文字列の場合は属性 0 件として扱う

## 予約 namespace

OTel が定義する Resource 属性との衝突を避けるため、`OTEL_RESOURCE_ATTRIBUTES` からは次の prefix のキーを **自動除外** します:

```
service.  host.  os.  process.  telemetry.  deployment.  cloud.  k8s.  container.
```

この除外は **`OTEL_RESOURCE_ATTRIBUTES` のみ** に適用されます。ユーザが明示的に書いた `--attr` や `SENKO_TRACE_ATTRIBUTES` の値はフィルタされません (明示指定はユーザ意図を尊重する)。

## 値サイズ制限

Remote 側で baggage 値を span attribute に昇格する際、**256 byte を超える値は UTF-8 境界で切り詰め** られ、`tracing::warn!` で警告ログが出ます。CLI 側では切り詰めしません。

## Remote (`senko serve`) が読む OTel 環境変数

Remote は起動時に次の標準 OTel 環境変数を読み、OTel SDK を初期化します。

| 変数 | 値 | 既定 | 挙動 |
|---|---|---|---|
| `OTEL_SDK_DISABLED` | `true` / `false` | `false` | `true` で OTel SDK 全層を無効化 (fmt ログ + W3C propagator のみ残る) |
| `OTEL_TRACES_EXPORTER` | `otlp` / `console` / `none` | `none` (※) | traces の送出先 |
| `OTEL_LOGS_EXPORTER` | `otlp` / `console` / `none` | `none` (※) | logs の送出先 |
| `OTEL_SERVICE_NAME` | 文字列 | `senko-server` | `service.name` Resource 属性の値 |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | URL | — | OTLP collector。これが設定されると exporter 既定が `otlp` に昇格 |
| `OTEL_RESOURCE_ATTRIBUTES` | `K=V,K=V,…` | — | OTel SDK の Resource 属性 (SDK が直接読む) |

> **(※) 既定値についての注意**: OTel 仕様の既定は `otlp` ですが、senko Remote は **OTel env が何も設定されていない時は `none`** にしています (ローカル開発で意図せぬ OTLP 接続を避けるため)。`OTEL_EXPORTER_OTLP_ENDPOINT` を設定すると exporter の既定が `otlp` に昇格します。未知の exporter 名は警告ログを出して `none` として扱います。

## Baggage → Span attribute の昇格

Remote 側のミドルウェア (`presentation/api/telemetry.rs` の `propagate_trace_context`) が、受信 `baggage` ヘッダの各エントリを **`baggage.<key>`** という名前の span attribute として span に付与します。

- **予約 namespace** のキー (`service.*` など上記リスト) は **防御的に除外** されます (CLI 側でもフィルタされるが、二重に守る)
- 値は **256 byte** で UTF-8 境界切り詰め
- 切り詰めが起きた時は `tracing::warn!` で警告

これにより、baggage で届いた `run.id=xyz` 等は Jaeger / Tempo 等のバックエンドで `baggage.run.id = "xyz"` として検索可能になります。

## Proxy モードの注意

`senko serve --proxy` は、上流 Remote への転送時に **新しい `traceparent`** を発行します (パススルーではなく再発射)。インバウンドの `baggage` は抽出されて上流への転送リクエストにも再発射されるため、CLI が発した `baggage.<key>` は上流 Remote のスパンにそのまま現れます。

- Relay 側では **予約 namespace の再フィルタを行いません** (CLI 側で既に整形済み。二重フィルタは `--attr` 等で明示指定されたキーを意図せず落とすことになるため)
- 上流 Remote の `propagate_trace_context` が受信した `baggage` を `baggage.<key>` span attribute に昇格する際、防御的に予約 namespace を除外するのは従来通り

## グレースフルシャットダウン

`SIGINT` (Ctrl-C) と `SIGTERM` を受けると、Remote は axum の in-flight リクエストを drain したあと、OTel の tracer / logger provider を flush してから終了します。telemetry 系のドロップは **明示的な flush 後** に起きるので、短寿命プロセスでも最後のスパンが送出されます。

## 関連

- [`--attr` グローバルフラグ](cli.md#グローバルオプション)
- [OTel Tracing 運用ガイド](../guides/tracing.md) — Claude Code との共存、Jaeger / Tempo / console exporter の検証手順、セキュリティ考慮
