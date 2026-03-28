---
id: layered-architecture-design
title: レイヤードアーキテクチャへのリファクタリング設計方針
description: 関数中心スタイルを維持しつつ、presentation / application / domain / infra の4層構造に分離する設計判断
tags:
  - architecture
  - refactoring
  - domain-driven-design
  - layered-architecture
  - testing
created_at: 2026-03-28
updated_at: 2026-03-28
---

## 概要

senkoのコードベースが大きくなったため、レイヤードアーキテクチャに移行する。
関数中心スタイルは維持しつつ、責務を明確に分離する。

## レイヤー構成

```
presentation → application → domain ← infra
                    ↓              ↑
               port (trait)    impl (struct)
```

- **domain層はどこにも依存しない**（traitでportを定義するだけ）
- **application層はdomainのtrait（port）に依存**し、具体実装は知らない
- **infra層はdomainのtraitを実装**する（依存の方向がdomain向き）
- **presentation層はapplication serviceを呼ぶだけ**で、domain/infraの詳細を知らない

### presentation層
- cli: サブコマンド定義、引数/環境変数/設定ファイルのパース（引数→環境変数→設定ファイル→デフォルトのフォールバック）
- api: Axumハンドラ（application serviceに委譲）
- web: HTMLレンダリング

### application層
- 権限制御
- ドメイン層の手続き的実行
- loggerなど業務への関わりが薄いportはここで定義

### domain層
- aggregate, repository, domain service, value object, entityからなる
- repositoryなど業務に深い繋がりのあるportはここで定義
- aggregateに所属するエンティティの操作はaggregate rootを通じて行う

### infra層
- portの実装（sqlite, dynamodb, http等）

## 主要な設計判断

### Repository traitの分割
現在の`TaskBackend`（20メソッド）を`TaskRepository`と`ProjectRepository`に分割する。
単一責任の原則に沿い、テストも書きやすくなる。

### Hooks（webhook）の配置
- **設定モデル**（HooksConfig等）→ domain/config
- **hook実行ロジック** → infra/hook
- **発火タイミングの制御** → application層

### Domain Serviceの命名
`~~Service`は役割が不明瞭になるため使用しない。
責務がそのまま名前になる命名を採用する。

- ✅ `CyclicDependencyValidator` — 依存関係の循環検出
- ✅ `ReadyTaskResolver` — 依存完了済みTodoタスクの判定
- ✅ `PriorityComparator` — next_task選出の優先度比較
- ❌ `DependencyService` — 何をするか不明瞭

### Task Aggregate
状態遷移ルールはaggregate rootのメソッドに閉じる。
application層がif文で遷移可否を判断するのはドメイン知識の漏洩。

```rust
impl Task {
    pub fn ready(&mut self) -> Result<()> { /* Draft -> Todo */ }
    pub fn start(&mut self, session_id: Option<String>) -> Result<()> { /* Todo -> InProgress */ }
    pub fn complete(&mut self) -> Result<()> { /* InProgress -> Completed */ }
    pub fn cancel(&mut self, reason: Option<String>) -> Result<()> { /* any -> Canceled */ }
}
```

## 移行方針

段階的に移行し、各段階でテストが通る状態を維持する。

1. **Phase 1**: domain層の作成（型、trait、aggregate）
2. **Phase 2**: infra層の作成（既存backend実装の移動）
3. **Phase 3**: application層の作成（main.rsからビジネスロジック抽出）
4. **Phase 4**: presentation層の整理（CLI/API/Webの分離）
5. **Phase 5**: クリーンアップ（旧ファイル削除、互換レイヤー除去）

## テスト方針

3層構成。中間テストはSQLite repositoryのみに絞る。

| レベル | 対象 | 境界 | 実行環境 |
|--------|------|------|----------|
| **unit** | domain層（状態遷移、循環検出、Priority比較等） | 外部依存なし | in-memory |
| **integration** | SQLite repository | domain集約の永続化・復元の正確性 | 実SQLite（`:memory:`） |
| **e2e** | CLI全パス | CLIプロセス → SQLite → ファイルシステム | 実プロセス |

### application層の中間テストを導入しない理由

- application層は薄い手続き（取得→domain呼び出し→保存→hook）で分岐が少なく、e2eで十分カバーできる
- hook発火はfire-and-forget（`std::process::Command` + `std::thread::spawn`）で成否を関知しない設計のため、mock検証の価値が低い

### SQLite repositoryの中間テストを導入する理由

- 複数テーブルJOIN（task + dod + scope + tags + dependencies）による集約復元の正確性はunit/e2eの間にバグが潜みやすい
- マイグレーション後のデータ整合性の検証が必要
- `:memory:`で高速に実行できる
