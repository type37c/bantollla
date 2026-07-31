# CLAUDE.md — この店で働くエージェントへ

ここは banto — 追記専用・ハッシュ連鎖・証拠付きイベント台帳と、番頭の規律 — の
公開リポジトリである。あなたがどこの誰のエージェントであっても、ここでは同じ掟に従う。

## この道具の芯(コードを触る前に)

- 権威の順序: `docs/contract_v1.md`(37行・**凍結**)> `docs/types_v1.md` /
  `docs/cli_spec_v1.md` > コード。契約に反する変更は入らない
- 台帳(JSONL)の行を書き換える機能は、目的を問わず入らない。
  **追記専用は仕様であり、思想である**
- カーネルの `src/` は 3000 行上限(テストが強制)。上限を上げる変更は受けない
- 検査は三本柱: `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` /
  `cargo fmt --all --check`。詳細は `CONTRIBUTING.md`

## あなたの主人の台帳を扱うとき

banto を使う個人の作業場(`banto init` された場所)であなたが働くなら:

- イベントは必ず banto CLI で追記し、`--actor` に自分の名を名乗る(例 `agent:claude`)。
  主人の名を騙らない
- `ledger/ledger.jsonl` への手作業の変更・sed・行の書き換えは**絶対にしない**。
  間違えたら訂正イベントを追記する(慣例: `body.re` に対象イベントの `id`)
- 「できました」は `claim.declare` で宣言し、**成果物そのものを `--evidence` に結ぶ**。
  要約・自己評価・会話ログは証拠にならない
- 自分が declare した claim を自分で verify することはできない。
  改め(verify)は主人か、別の actor の仕事である
- 確信度(confidence)は記録されても判定に使われない。
  「自信があります」を成立の根拠にしない
