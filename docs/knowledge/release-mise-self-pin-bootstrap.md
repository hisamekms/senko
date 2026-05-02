# Release workflow と mise.toml の self-pin の chicken-and-egg

## 症状

タグ push 後、`release.yml` の `build-web` ジョブが `jdx/mise-action@v2` のステップで以下のエラーで失敗する:

```
mise ERROR Failed to install github:hisamekms/senko@<NEW_VERSION>:
HTTP status client error (404 Not Found) for url
(https://api.github.com/repos/hisamekms/senko/releases/tags/hisamekms/senko@<NEW_VERSION>)
```

その結果、Rust バイナリは artifact 化されるが GitHub Release は作成されず (release ジョブは build-web の依存)、`senko-web-<version>.tar.gz` も同梱されない。

## 原因

`mise.toml` の `[tools]` で senko 自身を新バージョンに pin している:

```toml
[tools]
"github:hisamekms/senko" = "0.43.0"
```

`jdx/mise-action@v2` は mise.toml に書かれた **すべての** ツールを install しようとする。senko 0.43.0 はリリース中であり **このジョブが完走しないと存在しない** ため 404。

## 対応

`build-web` ジョブで `MISE_DISABLE_TOOLS` 環境変数で senko 自身の install を skip する:

```yaml
build-web:
  runs-on: ubuntu-latest
  env:
    MISE_DISABLE_TOOLS: github:hisamekms/senko
  steps:
    - uses: actions/checkout@v6
    - uses: jdx/mise-action@v2
    ...
```

`MISE_DISABLE_TOOLS` はカンマ区切りで複数指定可能。`build-web` は node/npm/tar しか使わないため senko 本体は不要。

## 教訓

- ツールチェーン定義に「自分自身」を含めるリポジトリで、リリースワークフローが mise/asdf 等を使う場合、自己 install を skip する仕組みが必須。
- `jdx/mise-action@v2` のデフォルトは「定義されたものを全部入れる」で、特定ツールだけ skip する公式オプションは無く、`MISE_DISABLE_TOOLS` で実現するのが定石。
- 同じ罠は `[tools]` に他リポジトリの非公開バージョンや、CI で取得不可能な外部リソースを書いた場合にも起こる。

## 関連 commit

- 初発: v0.43.0 タグ push 時に発生 (workflow run 25251757579)
- fix: `e90a504 fix(ci): skip senko self-install in build-web (release bootstrap)`
