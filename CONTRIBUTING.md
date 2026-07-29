# 貢献の作法 / Contributing

まだ店を開けたばかりの小さな道具です。issue は日本語・英語どちらでも歓迎します。

## ビルドとテスト

```sh
cargo test --workspace     # 全テスト(敵対的エッジテスト含む)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

CI はこの三本柱をそのまま走らせます。

## この家の掟(変更を出す前に)

1. **権威の順序**: `docs/contract_v1.md`(37行・凍結)> `docs/types_v1.md` /
   `docs/cli_spec_v1.md` > コード。契約に反する変更は、まず契約の版上げの議論から。
2. **カーネルの src/ は 3000 行上限**(テストが強制)。上限を上げる PR は受けません —
   何かを足すなら、何を足さないかを先に。
3. **バグ報告は再現から**: 可能なら「失敗するテスト」を書いて PR にしてください。
   それが最初の貢献として一番ありがたい形です。
4. 台帳ファイル(JSONL)の行を書き換える機能は、目的を問わず入りません。
   追記専用は仕様であり、思想です。

## ふるまい

短く: 敬意を持って。初心者の質問を歓迎し、悪意には寛容ゼロで臨みます。
違反を見たら issue か メンテナへの直接連絡で知らせてください。

Be kind. Beginner questions are welcome; bad-faith behavior is not tolerated.
