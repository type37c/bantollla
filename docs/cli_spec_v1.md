# banto CLI 仕様 v1

単一バイナリ `banto`。この文書が CLI の仕様の単一情報源であり、実装とREADMEはここから派生する。

## 全体規則

- グローバルフラグ: `--root PATH`(ワークスペース指定)、`--actor NAME`(書き手の上書き)
- ワークスペース発見: `--root` がなければ cwd から上へ `banto.toml` を探す。見つからなければエラー(`init` を案内する)
- 追記系コマンドは成功時に `✓ 記録 seq=N type=... id=<full id>` を1行出力する
- 機械可読出力: `--json` を持つコマンドは JSON を stdout へ出す(AI 執務層が使う)
- エラーは日本語で stderr へ、終了コード 1(使い方エラーは clap 既定の 2)
- 台帳の行を編集・削除する機能は存在してはならない(契約 不変量1)

## 設定 `banto.toml`(すべて省略可)

```toml
actor = "alice"          # 既定の書き手。無ければ $USER
stall_days = 4           # project の停滞閾値(日)
habit_stall_days = 2     # habit の停滞閾値(日)
wip_limit = 2            # 稼働中プロジェクトの上限(habit は数えない)

[paths]
ledger = "ledger/ledger.jsonl"
objects = "ledger/objects"
```

## コマンド

### 初期化・記録

| コマンド | 動作 |
|---|---|
| `banto init [--actor NAME]` | `banto.toml`、空の台帳、`ledger/objects/.gitkeep` を作る。`banto.toml` が既にあればエラー |
| `banto append TYPE [--body JSON] [--evidence FILE]... [--op KEY] [--store]` | 汎用追記。`--body` は JSON オブジェクト(既定 `{}`)。証拠ファイルは SHA-256 を取り、`uri` はルートからの相対パス。`--store` は `objects/<sha256>` へコピーし uri をそこへ向ける |
| `banto video FILE [--note TEXT] [--loop SLUG] [--store]` | `video.log`。body は `{filename, note?, loop?}`、evidence は動画ファイル。原則 `--store` しない(動画は大きい) |
| `banto video --sha256 HEX --filename NAME [--bytes N] [--note] [--loop]` | 原本が手元にないとき、撮った端末で計算したハッシュだけで記帳する(uri なし証拠)。端末側は `shasum -a 256 <ファイル>` |
| `banto transcript EVENT_ID (--file PATH \| --text TEXT)` | `video.transcript`。EVENT_ID が `video.log` でなければエラー。body `{video, text}`、evidence はテキストの SHA-256 |
| `banto note TEXT [--tag TAG]...` | `note.capture`。body `{text, tags?}` |
| `banto journal (--text TEXT \| --file PATH) [--sha256 HEX --filename NAME [--bytes N]] [--loop SLUG]` | `journal.entry`。朝の統括(媒体非依存、テキストが本体)。テキストのハッシュが冪等キー。原本(音声/動画)のハッシュは任意で添える |

### ループ(番頭の領分)

| コマンド | 動作 |
|---|---|
| `banto loop open SLUG --title T [--kind project\|habit] [--next ACTION] [--reopen]` | ループを開く。同じ slug が稼働中ならエラー。閉/保留/殺済みの再開は `--reopen` が必要 |
| `banto loop next SLUG ACTION` | 次の一手を差し替える(15〜30分粒度を促す文言をヘルプに) |
| `banto loop touch SLUG [--note TEXT]` | 進捗を記録 |
| `banto loop close SLUG [--outcome TEXT]` | 閉じる |
| `banto loop park SLUG [--reason TEXT]` | 保留場へ |
| `banto loop kill SLUG [--reason TEXT]` | 殺す |
| `banto loop list [--all] [--json]` | 一覧。既定は active のみ、`--all` で全状態 |

存在しない slug への next/touch/close/park/kill はエラー。

### 導出ビュー

| コマンド | 動作 |
|---|---|
| `banto brief [--json]` | 朝礼ビュー。稼働中ループを放置日数の降順で表示、停滞(project≥stall_days / habit≥habit_stall_days)に ⚠、次の一手、保留場の件数、WIP超過警告 |
| `banto health [--json]` | イベント総数、ループ状態別件数、ビデオ途絶日数、最後に閉じてからのイベント数、鐘(bells)の一覧。鐘が鳴っていれば終了コード 1 |
| `banto log [--type PREFIX] [--limit N] [--json]` | イベント一覧(時系列、既定は直近20件)。`--type` は前方一致 |
| `banto verify [--evidence] [--deep]` | 台帳全検査。`--evidence` はファイル存在、`--deep` はハッシュ再計算まで。問題があれば一覧して終了コード 1 |

### ゲート(claim ≠ verified)

| コマンド | 動作 |
|---|---|
| `banto gate declare TITLE [--loop SLUG] [--confidence F] [--evidence FILE]... [--store]` | `claim.declare`。**evidence が0個ならエラー**(契約 不変量2)。confidence は記録されるが「判定には使われない」と明示出力 |
| `banto gate check CLAIM_ID` | 宣言の証拠を deep 検査し、`claim.verify`/`claim.reopen` の有無と最新判定を表示。証拠が 0 件(不変量2違反)または解決しなければ終了コード 1。宣言と同一 actor の `claim.verify` は警告つきで判定から除外する(不変量3) |
| `banto gate verify CLAIM_ID --verdict pass\|fail [--reason TEXT] [--evidence FILE]...` | `claim.verify`。**宣言と同じ actor ならエラー**(契約 不変量3) |
| `banto gate reopen CLAIM_ID --reason TEXT` | `claim.reopen` |

## media_type の推定(拡張子ベース、最小)

mp4→video/mp4、mov→video/quicktime、m4a→audio/mp4、mp3→audio/mpeg、wav→audio/wav、
txt/md→text/plain、json→application/json、jsonl→application/jsonl、pdf→application/pdf、
png→image/png、jpg/jpeg→image/jpeg。不明は省略。
