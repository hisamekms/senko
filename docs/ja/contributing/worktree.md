# Worktree ワークフロー

senko の開発では **main ブランチで直接ファイルを編集しないルール** になっています。変更は必ず worktree 内で行い、マージ後に worktree を削除します。

`/workspaces/senko` が main、`/workspaces/senko/worktrees/*` が作業用の worktree という構成です。

## なぜ worktree ?

- **ビルド成果物が main の target/ を汚さない** — worktree ごとに独立した作業ツリー
- **複数タスクを並行して進めやすい** — `docs-v1` と `fix-auth-bug` を切り替えずに両方触れる
- **main を常にクリーンに保てる** — push 前の一時的なコミットが main 上に残らない
- **AI エージェントのサンドボックス化** — エージェントが main を直接いじる事故を防げる

## ツール

専用の `wth` スクリプトを使います:

```bash
./scripts/bin/wth <command> <name>
```

または Claude Code で `/wth` スキル経由でも呼べます (ただし現状一部の登録状態次第で、直接スクリプト呼び出しが確実)。

> **禁止事項**:
> - `git worktree add/remove` を **直接** 実行しない
> - Claude Code の `EnterWorktree` ツールを **使わない**
> - main ブランチで直接ファイルを編集しない

## 基本操作

`wth` が提供するサブコマンドは `add` / `rm` / `help` の 3 つのみ。一覧や切り替えは素の git / シェルで行います。

### 作成

```bash
./scripts/bin/wth add my-feature
```

- `worktrees/my-feature/` に新しい worktree が作られる
- `wth/my-feature` という branch が **新規作成** されて checkout される (既存ブランチを流用したい場合は `wth` は使えない。素の `git worktree add` で)

### 一覧

```bash
git worktree list
```

### 切り替え

```bash
cd worktrees/my-feature
```

### 削除

```bash
./scripts/bin/wth rm my-feature
```

- 内部で `git worktree remove --force` + `git branch -D wth/my-feature` を実行 — **マージ済みチェックはしない**ので、作業途中の worktree でも即座に消えます
- `.wth/hooks/rm/*.sh` が削除前に走る (存在すれば)

## 典型フロー

```bash
# 1. 作業開始 (main (= /workspaces/senko) から)
./scripts/bin/wth add fix-auth-bug
cd worktrees/fix-auth-bug
#   この時点で branch wth/fix-auth-bug にチェックアウト済み

# 2. 編集・コミット・push・PR
vim src/...
git add . && git commit -m "fix: auth"
git push origin wth/fix-auth-bug
gh pr create

# 3. PR が main にマージされたら
cd /workspaces/senko
git pull
./scripts/bin/wth rm fix-auth-bug
```

## main 側の取り扱い

- main には `/workspaces` 経由でアクセス
- main ブランチは read-only 前提 (merge 以外の直接 commit なし)
- Claude Code のセッションでは、プロジェクトルール上 main 側での編集作業は拒否される

## 複数 worktree の共存

- `worktrees/docs-v1` と `worktrees/fix-x` を同時に持っても問題ない
- それぞれ独立した `target/` を持ち、senko の SQLite も XDG 配下で別パス (`$XDG_DATA_HOME/senko/projects/docs-v1/data.db` vs `.../projects/fix-x/data.db`) になるので干渉しない
- 一方で **共通の `.git/`** を参照しているので、branch 操作の整合は git 側でちゃんと取れる

## トラブルシューティング

| 症状 | 対処 |
|---|---|
| `wth add` で `worktree already exists` | 既存 worktree を `wth rm` で消すか、別名を使う |
| worktree 内で `git status` が変 | branch 自体がおかしいので `wth rm` → `wth add` で作り直す方が早い |
| main に書き込みたくなった | そもそもやらない。worktree を作る |
| `worktrees/` をうっかり main に commit してしまった | `.gitignore` に `worktrees/` が入っているはず。追加済みの場合は `git rm -r --cached worktrees/` |

## スクリプトの中身を見たい

```bash
cat ./scripts/bin/wth
```

- git worktree コマンドの薄いラッパ
- 規約上の path (`worktrees/`) と branch 名 (`wth/<name>`) を強制
- `add` 時に `.wth/hooks/add/*.sh`、`rm` 時に `.wth/hooks/rm/*.sh` を lex 順に実行 (存在すれば)
- `WTH_DIR` env で worktree の置き場所を上書き可能
