---
id: tracing-just-one-rebuilder-bug
title: tracing-core 0.1.x の JustOne rebuilder で並列テストが失敗する罠
description: set_default 1 つだけでテストを書くと、別スレッドの先発 callsite 登録で Interest が永続的に Never に固定される
tags:
  - tracing
  - tracing-core
  - opentelemetry
  - testing
  - rust
created_at: 2026-04-25
updated_at: 2026-04-25
---

## 概要

`tracing-core 0.1.x` (`tracing 0.1.x`) の callsite interest cache は、
`set_default(subscriber)` を 1 つだけ使う構成で並列テストを書くと
race により `Interest::Never` が永続化され、subscriber が作動しない場合がある。

具体的には、senko の `emit_business_event!` 配線テスト
(`create_contract_emits_otel_log_record` 等) が `mise test` (cargo default
parallel) で flaky に fail する原因がこれだった。

## 詳細

### 何が起きていたか

```rust
#[tokio::test(flavor = "current_thread")]
async fn create_contract_emits_otel_log_record() {
    let (exporter, provider) = build_capture_provider();
    let subscriber = tracing_subscriber::registry().with(capture_layer(&provider));
    let _guard = tracing::subscriber::set_default(subscriber);
    ops.create_contract(...).await.unwrap();   // 内部で emit_business_event!
    let logs = exporter.get_emitted_logs().expect(...);
    // ↑ 「senko.contract.created」 が見つからない (空配列)
}
```

`tracing::subscriber::set_default` は thread-local の dispatcher を切り替える
だけで、emit が同じスレッド (current_thread runtime) から発火するのに、
captured ログが空になる。`--test-threads=1` では PASS、parallel では fail。

### 原因: `Dispatchers::rebuilder()` の `JustOne` 最適化

`tracing-core::callsite::dispatchers::Dispatchers` には、登録 dispatcher が
1 つだけのときに `Rebuilder::JustOne` を返す高速パスがある:

```rust
// tracing-core 0.1.36 src/callsite.rs
pub(super) fn rebuilder(&self) -> Rebuilder<'_> {
    if self.has_just_one.load(Ordering::SeqCst) {
        return Rebuilder::JustOne;
    }
    Rebuilder::Read(LOCKED_DISPATCHERS.read().unwrap())
}
```

そして `Rebuilder::JustOne::for_each` は、登録された dispatcher リストを
**無視して**、呼び出し元スレッドの `dispatcher::get_default()` を使う:

```rust
impl Rebuilder<'_> {
    pub(super) fn for_each(&self, mut f: impl FnMut(&dispatcher::Dispatch)) {
        let iter = match self {
            Rebuilder::JustOne => {
                dispatcher::get_default(f);  // ← 呼び出しスレッドの thread-local
                return;
            }
            ...
        };
    }
}
```

`DefaultCallsite::register()` (callsite 初回登録時) はこの `rebuilder()` を
使って interest を計算する。並列実行で:

1. テスト A (subscriber 設定済み) がスレッド T1 で動作中、`has_just_one = true`
2. テスト B (subscriber 未設定) がスレッド T2 で `create_contract` を呼ぶ
3. T2 で `emit_business_event!` が初回発火、`CALLSITE.interest()` が `register()` を呼ぶ
4. `register()` が `DISPATCHERS.rebuilder()` → `JustOne` → T2 の `get_default()`
5. T2 の thread-local default は `NoSubscriber` → `Interest::Never` を返す
6. callsite の interest 静的領域に **`INTEREST_NEVER` が永続的に書き込まれる**
7. テスト A が emit を発火しても、cache が `Never` のため macro が短絡してスキップ

つまり「自分が唯一の subscriber」設計が、**自分のロジックを壊す** という直感に
反する race。

### 解決策: ダミー dispatcher を OnceLock で永続保持

「常に dispatcher が 2 つ以上登録されている」状態を維持すれば、
`has_just_one = false` になり、`Rebuilder::Read(vec)` が選択される。
これは登録された dispatcher リストを実際に iter するため、テスト用 subscriber の
interest が正しく問い合わされる。

senko の `application::telemetry::test_support` ではこれを
`ensure_dispatch_anchor()` という helper で実装している:

```rust
fn ensure_dispatch_anchor() {
    static ANCHOR: OnceLock<tracing::Dispatch> = OnceLock::new();
    ANCHOR.get_or_init(|| {
        tracing::Dispatch::new(tracing_subscriber::Registry::default())
    });
}
```

`build_capture_provider()` の最初に `ensure_dispatch_anchor()` を呼ぶことで、
プロセス起動後の最初の OTel テストで anchor が固定される。以降 `OnceLock` が
Dispatch を保持するため `has_just_one = false` が維持される。

`tracing::Dispatch::new(...)` を呼ぶこと自体が `register_dispatch` 経由で
`LOCKED_DISPATCHERS` に追加するため、明示的な register 呼び出しは不要。

### 適用条件

以下の条件が **重なった時** にこの bug が顕在化する:

- tracing-core 0.1.x の `Dispatchers::rebuilder()` の `JustOne` 最適化
  (上流で fix されるまで継続)
- 「テスト用 subscriber を `set_default` で 1 つだけ立てる」設計
- 同一プロセスで「subscriber を立てない」テストが並列で同じ callsite を発火する
- `current_thread` runtime + sqlx `spawn_blocking` 等で、await 越しに macro が発火

`with_default(subscriber, || { ... })` も内部実装は `set_default` と等価
なので、この race の解決にはならない (確認済)。`WithSubscriber` (tracing-futures)
も無効。

### 関連

- 上流 issue (もしあれば): tokio-rs/tracing の "callsite cache pollution"
  系の議論
- `serial_test` で OTel テスト群を排他化する案も検討したが、anchor 方式の方が
  diff が小さく、テスト並列性も維持できるため senko ではこちらを採用
