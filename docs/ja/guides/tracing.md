# OTel Tracing 運用ガイド

CLI → Remote の任意属性伝搬と、Remote 側 OTel SDK での traces / logs 送出を、実際のシェルで動かすためのガイド。

仕様の全量は [Tracing リファレンス](../reference/tracing.md) を参照。

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

- `deployment.environment` / `team` など **予約 namespace でないキー** は `OTEL_RESOURCE_ATTRIBUTES` から senko が拾って baggage に乗せる → Remote 側でも `baggage.team = "backend"` として span 属性化される。
- `service.name` など **予約 namespace のキー** は senko の baggage には乗らない。Claude Code / senko それぞれの OTel SDK が **Resource 属性として** 直接読むため、別ルートで同じ値がバックエンドに届く。

## ローカル検証: console exporter で中身を見る

collector を立てる前に、まず SDK が動くことを目視で確認したい時:

```bash
OTEL_TRACES_EXPORTER=console \
OTEL_LOGS_EXPORTER=console \
senko serve
```

stdout に span / log が JSON で落ちます。別ターミナルから:

```bash
senko --attr run.id=demo1 task list
```

を実行し、サーバ側 stdout の span に `baggage.run.id = "demo1"` が載っているか目視確認します。

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

`http://localhost:16686` を開き、`Service = senko-dev` で検索。span の attribute に `baggage.run.id=demo1` / `baggage.user.slot=alice` が出ているはずです。

## Tempo で可視化する

Tempo も OTLP gRPC を受けるので、エンドポイント URL を差し替えるだけで同じ手順が使えます。Grafana から Tempo datasource を繋ぎ、`baggage.run.id="demo1"` 等で TraceQL 検索できます。

## 検証チェックリスト

実装が期待通りに動いているか確認する最小項目:

- [ ] Remote 側リクエストログに `baggage` ヘッダの値が流れている
- [ ] Span に `baggage.<key>` attribute が付いている
- [ ] `OTEL_RESOURCE_ATTRIBUTES` に `service.name=foo` を入れても、それが baggage 経由では **Remote の span に出てこない** こと (予約 namespace 除外)
- [ ] `--attr` と `OTEL_RESOURCE_ATTRIBUTES` に同じキーを入れると **`--attr` の値が勝つ** こと
- [ ] `OTEL_SDK_DISABLED=true` 下では console / OTLP どちらの exporter も無音になること (fmt ログは残る)
- [ ] 1 値 256 byte 超の baggage を送ると Remote のログに `tracing::warn!` の切り詰め警告が出ること

## 無効化したいとき

完全に止めたい時:

```bash
OTEL_SDK_DISABLED=true senko serve
```

これで collector への発呼も console 出力もゼロになります (fmt ログは残る)。部分的に止めたい時は exporter 側で:

```bash
OTEL_TRACES_EXPORTER=none OTEL_LOGS_EXPORTER=none senko serve
```

## セキュリティ考慮

- **認可には使わない**。 baggage / resource attrs は CLI 側で自由に書けます (`--attr`・任意の env)。Remote 側の権限判定 (user id の推定、role の解釈など) に使ってはいけません。認可は必ず OIDC / API key / 信頼ヘッダなど **認証済み identity** 経由で行ってください。
- **PII を載せない**。 baggage は HTTP ヘッダとしてワイヤに乗り、span / log としてバックエンドに **長期保存** されます。メールアドレス・パスワード・API token・リクエスト本文などは値にしないこと。
- **値サイズを意識する**。 256 byte 超は Remote 側で切り詰められ、warn ログが出ます。切り詰め前の値を前提にクエリや集計を組まないこと。
- **不正値はサイレントに消える**。 `OTEL_RESOURCE_ATTRIBUTES` / `SENKO_TRACE_ATTRIBUTES` の malformed エントリは OTel spec に従って **silently skip** されます。設定ミスに気付きたい時は代わりに `--attr` を使ってください (malformed ならエラー終了します)。

## 関連

- 仕様全量: [Tracing リファレンス](../reference/tracing.md)
- Remote のデプロイ: [server-remote デプロイ](server-remote/deploy.md)
