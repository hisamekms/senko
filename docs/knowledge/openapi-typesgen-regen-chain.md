---
id: openapi-typesgen-regen-chain
title: openapi.json と types.gen.ts は生成物 — DTO変更後は dump → gen:api の順で再生成
description: APIスキーマ（utoipa derive）を変更したら docs/openapi/openapi.json と web/src/api/types.gen.ts をコミット前に再生成する必要がある。並行ビルド中の dump は古いバイナリで走るリスクがある。
tags:
  - openapi
  - utoipa
  - codegen
  - web
created_at: 2026-07-26
updated_at: 2026-07-26
---

## 概要

`docs/openapi/openapi.json` と `web/src/api/types.gen.ts` はどちらも**コミットされる生成物**。
DTO（`#[derive(utoipa::ToSchema)]` を持つ型や `#[utoipa::path]` アノテーション）を変更しても自動では更新されないため、手動で再生成してコミットする必要がある。

## 詳細

生成チェーンは一方向:

```
utoipa アノテーション (src/presentation/{api,dto}.rs, domain の ToSchema 型)
  → senko openapi dump          # docs/openapi/openapi.json を書き出し（実行時生成、build.rs ではない）
  → cd web && npm run gen:api   # openapi-typescript が types.gen.ts を生成
```

- OpenAPI 仕様は `build_openapi()`（`src/presentation/api/mod.rs`）が実行時に組み立てる。`senko openapi dump` の既定出力先が `docs/openapi/openapi.json`。
- `npm run gen:api` は `openapi-typescript ../docs/openapi/openapi.json -o src/api/types.gen.ts`。

### 落とし穴: 並行 cargo ビルド中の dump は古いバイナリで走ることがある

`cargo run -- openapi dump` を別プロセスの cargo ビルド（サブエージェントの `cargo check` / `mise test` など）と同時に走らせたところ、**DTO変更前の古いバイナリで dump が実行され、新フィールドが欠けた spec が生成された**（差分が3行しかなく気づけた）。dump 後は目的のスキーマ変更が spec に含まれているか必ず確認する:

```bash
python3 -c "import json; s=json.load(open('docs/openapi/openapi.json'))['components']['schemas']; print(list(s['DodItemResponse']['properties']))"
```

## 解決策 / 推奨事項

1. DTO変更をコンパイルが通る状態にする
2. 他の cargo プロセスが走っていないタイミングで `cargo run -- openapi dump`
3. spec に変更が反映されたことを確認（上記ワンライナー等）
4. `cd web && npm run gen:api`（node_modules がなければ `npm ci --legacy-peer-deps`、[[web-npm-legacy-peer-deps]] 参照）
5. `openapi.json` と `types.gen.ts` を実装と同じコミットに含める

## 参考

- `web/package.json` の `gen:api` スクリプト
- docs/knowledge/web-npm-legacy-peer-deps.md
