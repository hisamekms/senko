---
id: hook-fire-task-none-noop
title: hook fire() は task=None だと早期 return するため task_add の pre-hook は発火しない
description: HookTrigger::Task で task が None の場合、fire() は envelope を組み立てられず event_fired ログすら書かずに Continue を返す。task_add / task_select(next) の pre-hook はこの経路に該当し、abort も効かない。
tags:
  - hooks
  - pre-hook
  - task-add
  - abort
created_at: 2026-07-21
updated_at: 2026-07-21
---

## 概要

`src/infra/hook/mod.rs` の `fire()` は `HookTrigger::Task(_)` かつ `task == None` のとき、envelope を構築できないため **`event_fired` ログも書かずに `FireOutcome::Continue` を早期 return** する。

このため、遷移前のタスクオブジェクトが存在しない以下のイベントでは `when = "pre"` の hook は runtime を問わず（local / remote どちらでも）実質発火しない:

- `task_add`（作成前なのでタスクが存在しない）
- `task next` の `task_start` pre（remote 経路では選択がサーバー側で行われるため、開始対象タスクが事前に不明）

## 詳細

- 呼び出し側（`LocalTaskOperations::create_task` / `RemoteTaskOperations::create_task`）は「preview task が無いので None を渡す」というコメント付きで `fire(&trigger, HookWhen::Pre, None, ...)` を呼んでいるが、`fire()` 側のガード（`let Some(task) = task else { return Continue }`）により hook 実行に到達しない。
- したがって `task_add` に `when = "pre"` + `mode = "sync"` + `on_failure = "abort"` を設定しても作成をブロックできない。
- `task_publish` / `task_start` / `task_resume` / `task_complete` / `task_cancel` は遷移前に `get_task` で prev を取得して `Some(&prev)` を渡すため、pre-hook + abort が正しく機能する。

## 解決策 / 推奨事項

- 作成そのものをゲートしたい場合は、task_add の pre-hook ではなくサーバー側（`[server.remote.task_add.hooks]`）でも同様に効かないため、現状は API 層の手前（認可・別バリデーション）で行うしかない。
- 将来 task_add pre-hook を有効化するには、`fire()` が task 無しでも envelope（task フィールド null）を構築できるようにする改修が必要。
- デバッグ時の見分け方: `senko hooks log` に `event_fired` が **出ていない** 場合はこの早期 return 経路（または runtime 不一致）を疑う。`event_fired` が出て `hook_ok`/`hook_failed` が無い場合は `when`/`on_result` フィルタ不一致を疑う。

## 参考

- `src/infra/hook/mod.rs` の `fire()`（`HookTrigger::Task` の match arm）
- `src/application/task_service.rs` / `src/infra/http/remote_task_ops.rs` の `create_task`
