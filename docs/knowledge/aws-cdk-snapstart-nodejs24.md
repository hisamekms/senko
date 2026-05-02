# aws-cdk-lib `Runtime.NODEJS_24_X.supportsSnapStart` is still `false` (2.252.0)

## 観測

aws-cdk-lib **2.252.0** (npm `latest` を 2026-05-02 時点で確認) の
`Runtime` メタデータでは、Node.js 系すべてのランタイムで
`supportsSnapStart` がまだ `false` のまま:

```js
const Runtime = require('aws-cdk-lib/aws-lambda').Runtime
// NODEJS_18_X { name: 'nodejs18.x', supportsSnapStart: false }
// NODEJS_20_X { name: 'nodejs20.x', supportsSnapStart: false }
// NODEJS_22_X { name: 'nodejs22.x', supportsSnapStart: false }
// NODEJS_24_X { name: 'nodejs24.x', supportsSnapStart: false }
```

AWS Lambda 側は Node 20+ ランタイム (Node 24 を含む) で SnapStart GA
済みだが、CDK の高レベル API は `supportsSnapStart` フラグで synth-time
バリデーションをかけているため、`new Function(..., { snapStart:
SnapStartConf.ON_PUBLISHED_VERSIONS })` を渡すと
`Runtime nodejs24.x does not support SnapStart` で synth が落ちる。

## 影響

`docs/{ja,en}/guides/web/aws-lambda-cognito.md` のサンプル CDK スタックは、
高レベル `snapStart` プロップではなく `addPropertyOverride('SnapStart',
{ ApplyOn: 'PublishedVersions' })` で **CFN プロパティを直接** 指定する
ワークアラウンドを継続している。合成された CloudFormation テンプレートは
高レベル API を使った場合と同一なので、運用上の差は無い。

## 撤去トリガー

CDK 側で以下のいずれかが起きた時点で override を外し、高レベル
`snapStart: SnapStartConf.ON_PUBLISHED_VERSIONS` に戻してよい:

- aws-cdk-lib のリリースノートで `Runtime.NODEJS_24_X` (もしくはその基底
  `RuntimeFamily.NODEJS`) の `supportsSnapStart` が `true` に変わった旨が
  記載される。
- 上記スニペットを再実行して `supportsSnapStart: true` を確認できる。

両方の deploy guide の SnapStart 周りのコメントとポストスクリプト箇条書き
からも同時にワークアラウンド説明を削除すること。

## 関連

- AWS Lambda Node 24 runtime GA: <https://aws.amazon.com/about-aws/whats-new/2025/>
  (公式アナウンス。Node 20+ は SnapStart 対応済み)
- CDK 側の Runtime メタデータ: aws-cdk-lib `aws-lambda/lib/runtime.ts` の
  `RuntimeFamily.NODEJS` セクションで各 `Runtime` 定義の第3引数に
  `{ supportsSnapStart: true }` が付くまで待つ。
- senko task #423 (`chore: bump senko-web Node baseline to 24`) で
  ワークアラウンドを `NODEJS_24_X` 向けに継続する判断を確定。
