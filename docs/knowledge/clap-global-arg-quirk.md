---
id: clap-global-arg-quirk
title: clap の `#[arg(global = true)]` × 繰り返しフラグの集約挙動
description: global かつ Vec の repeated flag は、サブコマンド前後の出現を集約しない（最後にマッチした位置の値のみが残る）
tags:
  - clap
  - cli
  - rust
created_at: 2026-04-29
updated_at: 2026-04-29
---

## 概要

clap derive で `#[arg(global = true)]` を付けたフラグは、サブコマンドの前後どちらに置いても受理される。ただし `Vec` 型の repeated flag を **両方の位置で同時に** 使うと、すべてが集約されるわけではなく、最後にマッチしたサブコマンドレベルの値のみが derive 構造体に格納される。

## 詳細

例: `Cli` 構造体に `#[arg(long = "attr", global = true)] pub attr: Vec<(String,String)>` を定義した状態で、

```sh
senko --attr a=1 task list --attr b=2
```

を実行した場合、`cli.attr` は `[("a","1"), ("b","2")]` ではなく `[("b","2")]` になる。

これは clap の global arg 実装が、各サブコマンド階層に独立した `ArgMatches` を作る一方、derive マクロは「最も内側にマッチしたサブコマンドの ArgMatches」から値を読み取るため。トップレベルの `--attr a=1` は「親階層の ArgMatches」に格納されるが、derive は子階層の値で上書き参照してしまう。

ただし、

- すべてサブコマンド前: `senko --attr a=1 --attr b=2 task list` → `[a=1, b=2]` ✓
- すべてサブコマンド後: `senko task list --attr a=1 --attr b=2` → `[a=1, b=2]` ✓

のように **片方の位置に集約していれば** 期待通り動く。

## 対応方針

senko では `--attr` の position 集約はサポートせず、ユーザに「すべて前 or すべて後ろ」を期待する運用としている。これは元々（`global = false` 時代）「すべて前」しか許されなかった挙動の自然な拡張であり、後方互換性も保たれる。

cf. [task #387](https://github.com/) — トップレベルフラグを `global = true` にした時の検討。

## 関連

- 単純なスカラー型（`bool`, `Option<T>`, `OutputFormat` など）は global = true で前後どちらでも問題なく動く（後置の値が前置の値を上書きする clap の通常挙動）。
