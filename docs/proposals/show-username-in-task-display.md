# Proposal: task / contract 表示で UserId と一緒に username を表示する（CLI + Web）

Status: **Accepted** — Option B（API DTO 拡張）を採用
Date: 2026-05-03
Related Task: #431

## 背景

Task aggregate は `assignee_user_id: Option<UserId>`（内部 DB ID, `i64`）のみを保持しており、CLI の text 出力と Web の task detail ページがその数値 ID を生のまま表示している。`User` aggregate は `username: Username`（人間が識別しやすい handle）を持っているため、表示時には ID + username の併記が望ましい。Contract 側は contract notes に author を持たず、現状 user 参照は無いことを inventory で確認した。

## Inventory（棚卸し）

### CLI（Rust, `src/presentation/cli/`）

| 場所 | 出力 | 現在の表示 | 種別 |
|---|---|---|---|
| `src/presentation/cli/handlers.rs:cmd_get` | text | `Assignee (user): #{uid}` | task get |
| `src/presentation/cli/handlers.rs:cmd_get` | JSON | **生の `Task` aggregate を `serde_json::to_string_pretty(&task)` で serialize**（DTO 経由ではない）。`assignee_user_id` フィールドは含まれるが presentation DTO 側に新設する `assignee` は含まれない | task get |
| `src/presentation/cli/handlers.rs:cmd_list` | JSON | task list の各 item も生の Task 集約 | task list |
| `src/presentation/cli/handlers.rs:cmd_list` | text | task list は `[status] #id title (priority)` のみ — assignee は **表示していない** | task list |
| その他 task add / start / next / complete / publish / cancel / edit などの `--output json` | JSON | 同上、生 Task 集約を直接 serialize | 各 task サブコマンド |
| contract get / list / note add / list | text + JSON | **user_id を含むフィールドなし**（`ContractNoteResponse` は `content`/`source_task_id`/`created_at` のみ） | — |

> **重要**: CLI と HTTP API は task の JSON 形を一致させていない。HTTP API は `TaskResponse` (presentation DTO) を返すが、CLI は `Task` (domain aggregate) を直接 serialize している。本タスクのスコープでは **`task get` のみ DTO 経由に変える**（DoD「CLI で senko task show を実行し、username が表示される」を最小コストで満たすため）。他の CLI コマンドの JSON 整合は follow-up とする。

参考（本タスクの対象外、すでに username を併記している既存箇所）:
- `senko user list/show` （`handlers.rs:2055-2069`）
- `senko project members list` （`handlers.rs:2163-2183`）

### Web（TS/React, `web/src/`）

| 場所 | 表示 | 現在の取り扱い |
|---|---|---|
| `web/src/routes/_authed/p/$projectId/tasks/$id.tsx:324-328` | `<span>#{task.assignee_user_id}</span>` （task detail ページの assignee 行） | username lookup なし、生 ID 表示 |
| `web/src/components/tasks/TaskSummaryCard.tsx` | assignee を表示していない | — |
| `web/src/routes/_authed/p/$projectId/tasks/index.tsx` | TaskSummaryCard 経由のため非表示 | — |
| `web/src/routes/_authed/p/$projectId/contracts/$id.tsx`, `web/src/components/contracts/ContractNotesTimeline.tsx`, `ContractSummaryCard.tsx` | contract 系画面に user_id 表示は **なし** | — |

### API DTO

| DTO | 場所 | user_id フィールド | 既存の username 併記 |
|---|---|---|---|
| `TaskResponse` | `src/presentation/dto.rs:78-141` | `assignee_user_id: Option<UserId>` | なし（本提案で追加対象） |
| `ContractNoteResponse` | `src/presentation/dto.rs:145-166` | なし | — |
| `ContractResponse` | `src/presentation/dto.rs:168-205` | なし | — |
| `ProjectMemberResponse` | `src/presentation/dto.rs:445-466` | `user_id: UserId` | **`user: Option<MemberUserInfo>` ですでに併記済み**（先例パターン） |

`MemberUserInfo` は `{ id, name, display_name }` を持ち、`From<&User>` 実装で User 集約から組み立てる。`from_parts(member, user: Option<&User>)` パターンが既に存在する。

### user lookup primitives

- `UserRepository::get_user(id)` — 単発 lookup（`src/domain/user.rs:662-669`）
- `UserQueryPort::list_users(filter)` — page 付き list（`src/application/port/user_query.rs`）
- **bulk by-ids（`list_by_ids`）API は存在しない** — 必要なら追加が必要
- HTTP API: `GET /api/v1/users/{user_id}` は **`require_master(&auth)` が掛かっており、master role 以外は 403**（`src/presentation/api/mod.rs:2615-2620`）。`GET /api/v1/users` も同様に master-only。
- `Username` value object: 非空・最大 100 文字（`src/domain/user.rs:215-306`、`src/domain/validator.rs:14`）

## 設計案（2〜3 案）

### Option A — Presentation-layer lookup（フロント / CLI で都度 user 取得）

CLI: `task get` の text 描画前に handlers が `UserRepository::get_user(uid)` を呼び、`Assignee (user): #{uid} <username>` のように合成する。
Web: task detail ページが `apiClient.GET('/api/v1/users/{user_id}')` を fire-and-render する。

| 観点 | 評価 |
|---|---|
| 実装複雑度 | **低**（DTO もスキーマも変えない） |
| 性能 | task list では N+1。task detail は 1 query 追加で許容範囲 |
| API 後方互換 | **完全互換**（出力 JSON / OpenAPI 不変） |
| データ整合性 | username rename に即追従（lookup 時点の値） |
| **ブロッカー** | **`GET /api/v1/users/{user_id}` が master-only** のため、Web 側で project member（非 master）が呼ぶと 403。現状のままでは Web で実現不可。authz を緩める or 新エンドポイントを追加するなら別タスク・別議論が必要 |

### Option B — API DTO 拡張（server-side で username を併記）

`ProjectMemberResponse` の先例 (`user: Option<MemberUserInfo>`) を踏襲し、`TaskResponse` にも user companion を持たせる。HTTP / CLI 共通の DTO 変換時に server side で resolve する。

具体的な追加形（ProjectMember とフィールド名を揃える）:

```rust
pub struct TaskResponse {
    // ... 既存フィールド ...
    assignee_user_id: Option<UserId>,        // 既存（保持）
    assignee: Option<MemberUserInfo>,        // ★追加: id + name + display_name
}
```

`TaskResponse::from_parts(task, assignee_user: Option<&User>)` を新設（既存 `From<Task>` は保持し、assignee 解決を呼び出し側に委譲する形にしておく）。CLI / HTTP のクエリハンドラが、結果 Task の `assignee_user_id` を集めて `UserRepository::list_by_ids(&[UserId])` のような bulk lookup でまとめて取得し、Map にして DTO 組立に渡す。

| 観点 | 評価 |
|---|---|
| 実装複雑度 | **中**（DTO 追加、`UserQueryPort::list_by_ids` 追加 + sqlite/postgres 実装、CLI/HTTP のハンドラを改修） |
| 性能 | list でも 1 round trip + 1 bulk SQL（`WHERE id IN (...)`）で済む |
| API 後方互換 | **完全互換（additive）**。`assignee_user_id` は据え置き。新フィールド `assignee` は optional。OpenAPI 生成 → web 型再生成のみ |
| データ整合性 | 都度 resolve なので rename 即追従 |
| Web 側 | `task.assignee?.name` を直接表示。新規 fetch 不要。authz 問題も発生しない（task の参照権限と同じ） |

### Option C — Denormalization（task テーブルに username をキャッシュ）

`tasks.assignee_username TEXT` カラムを追加し、`task start` / `task edit --assignee` 時にユーザー名を写しておく。User rename 時には全タスクの username を一括更新する仕組みも必要。

| 観点 | 評価 |
|---|---|
| 実装複雑度 | **高**（migration + user rename フローへの追従コード + ドメイン境界の越境） |
| 性能 | 読み取りはゼロコスト |
| API 後方互換 | DTO に新フィールド追加（B と同じく additive） |
| データ整合性 | **rename 反映が遅延 / 漏れリスク**。user 集約の責務が task 側に染み出す |
| その他 | DDD の集約境界の観点で過剰結合。User rename は今のところ稀という前提なら overkill |

## 推奨

**Option B**。

- 既に `ProjectMemberResponse.user: Option<MemberUserInfo>` という同型の先例が dto.rs にあり、命名・パターン・OpenAPI 生成が一貫する。
- master 権限を要する `GET /users/{id}` を介さずに済むので Web 側の authz 衝突を回避。
- `list_by_ids` を一度だけ追加すれば task 以外（将来 contract notes に author を足したくなったときなど）にも再利用できる。
- 後方互換は additive。`assignee_user_id` は据え置き、`assignee` は optional。

## JSON / API DTO 後方互換方針（Option B 採用前提）

- 既存の `assignee_user_id: Option<UserId>` フィールドは **保持**（削除しない、deprecated にもしない）。スクリプト互換と内部 ID 直参照のため。
- 新フィールド `assignee: Option<MemberUserInfo>` を **additive** に追加。
  - `null` if assignee_user_id is None or user lookup failed (例: 削除済 user)。lookup 失敗時のハンドリングは「null を返して fall back」する方針（クラッシュさせない）。
- OpenAPI 自動生成 → `web/src/api/types.gen.ts` 再生成（mise task で）。
- CLI text 出力は `Assignee (user): #{uid} <username>` 形式に変更。username 不明時は従来どおり `#{uid}` のみ。
- CLI JSON 出力は新スキーマに従う（追加のみ、既存フィールド据え置き）。

## 実装範囲（Option B 採用前提）

bulk lookup（`list_by_ids`）は **追加しない**。既存の `list_members` ハンドラ（`src/presentation/api/mod.rs:2739`）がすでに per-item `state.user_service.get_user(member.user_id()).await.ok()` で resolve しており、同じパターンで揃える。SQLite の in-process roundtrip コストは小さく、200 件ページでも体感差は出ない。将来必要になれば `list_members` と合わせて bulk 化すれば良い（別タスク）。

| Layer | 変更 |
|---|---|
| presentation/dto (`src/presentation/dto.rs`) | `TaskResponse` に `assignee: Option<MemberUserInfo>` を追加。`TaskResponse::from_parts(task: Task, assignee_user: Option<&User>)` を新設。既存 `From<Task>` は internal な `assignee=None` 経路として保持（または呼び出し元を全て `from_parts` に置換）。`CompleteTaskResponse::from(CompleteResult)` も `from_parts` 受け渡しに改修 |
| presentation/api (`src/presentation/api/mod.rs`) | `TaskResponse` を返す全ハンドラ（list / get / publish / start / resume / complete / cancel / edit / next, contract から派生する分も含む — 1324 / 1398 / 1425 / 1551 / 1631 / 1668 / 1706 / 1737 / 1768 / 1819 / 1906 / 1938 / 1969 / 1999 / 2031 / 2063）で `state.user_service.get_user(uid).await.ok()` → `TaskResponse::from_parts` の組み立てに置換 |
| presentation/cli (`src/presentation/cli/handlers.rs`) | text 出力（`task get` line 455）を `Assignee (user): #{uid} <username>` に変更。JSON は DTO 経由（remote mode で API、local mode は in-process backend を経由するため、local の場合は CLI 側で別途 `UserOperations::get_user` を呼んで合成する必要がある — 既存 user_service ハンドルにアクセス可能） |
| web (`web/src/api/types.gen.ts`) | OpenAPI 再生成（`mise run web:gen-types` 等のタスクが既存。なければ生成スクリプト確認） |
| web (`web/src/routes/_authed/p/$projectId/tasks/$id.tsx`) | `task.assignee?.name` を表示。fallback として `#{task.assignee_user_id}` |
| tests | unit (dto.rs 内 `TaskResponse::from_parts` のテスト) + e2e (`tests/e2e/test_assignee_user_id.sh` 系のスクリプトに username 表示の assertion を追加) |

### 命名検討

`MemberUserInfo` は元々 ProjectMember 用に名付けられたが、構造（id + name + display_name）は task assignee にもそのまま流用可能。リネームの選択肢:

- **(I) そのまま再利用**: 命名が "Member" 寄りで微妙だが OpenAPI スキーマは増えず、既存利用箇所への影響もゼロ。**本タスクではこちらを採用**。
- (II) `UserBriefInfo` 等にリネーム: 命名は綺麗だが ProjectMember 系の OpenAPI 名が変わり、Web 側の generated types diff が広がる。本タスクのスコープ外として deferred。

## 採用案

**Option B: API DTO 拡張** を採用（2026-05-03、AskUserQuestion でユーザー選択）。

- `TaskResponse` に `assignee: Option<MemberUserInfo>` を additive に追加。`assignee_user_id` は据え置き。
- bulk lookup（`list_by_ids`）は導入せず、既存 `list_members` ハンドラと同じ per-item `state.user_service.get_user(uid).await.ok()` パターンを再利用。
- CLI text 出力は `Assignee (user): #{uid} <username>` 形式に拡張。username 不明（user 削除済み等）時は従来どおり `#{uid}` のみ。
- CLI JSON は **`task get` のみ** DTO (`TaskResponse`) 経由に変更し、新フィールド `assignee` を含む形に揃える。`task list` / `task add` / `task next` / `task start` / `task complete` などは生 Task の serialize を継続（follow-up で整合可能）。
- Web は `task.assignee?.name` を表示し、未取得・null の場合は `#{task.assignee_user_id}` に fall back。
- 後方互換: 既存 `assignee_user_id` フィールドは保持。新フィールド `assignee` は optional・additive のため破壊的変更なし。

### Follow-up

- 他 CLI サブコマンド（`task add` / `task list` / `task start` / `task complete` / `task next` 等）の JSON 出力も `TaskResponse` 経由に統一する。スコープを切り出して別タスク化推奨。
