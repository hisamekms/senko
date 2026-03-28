# 開発ガイド

[English](DEVELOPMENT.md)

## ステータス遷移

```
draft → todo → in_progress → completed
                    ↓
                 canceled
```

- `draft` → `todo` → `in_progress` → `completed`: 前方遷移のみ
- アクティブな状態 → `canceled`: 常に可能
- 後方遷移・自己遷移は不可

## データ保存

データベースは `<プロジェクトルート>/.senko/data.db` に自動作成されます。

プロジェクトルートは `.senko/`、`.git/` の存在で自動検出されます（カレントディレクトリにフォールバック）。

## テスト

```bash
cargo test                    # ユニットテスト
bash tests/e2e/run.sh         # E2Eテスト
```
