# banto 語彙 v1

契約(contract_v1.md)は封筒だけを凍結する。この文書は `type` と `body` の**現行の慣例**であり、
契約と違って追記的に育つ。既存 type の意味を変えるときは新しい type を切る。

## ループ管理(番頭の領分)

開いたループ = 閉じるまで注意を要求し続ける仕事の単位。slug で識別する。

| type | body | 意味 |
|---|---|---|
| `loop.open` | `{slug, title, kind?, next?}` | ループを開く。`kind` は `project`(既定) か `habit`。`next` は次の一手 |
| `loop.next` | `{slug, action}` | 次の一手を差し替える。**15〜30分で終わる粒度**にする |
| `loop.touch` | `{slug, note?}` | 進捗があったことを記録する |
| `loop.close` | `{slug, outcome?}` | 完了して閉じる。番頭の存在意義 |
| `loop.park` | `{slug, reason?}` | 意識的に保留場へ送る(黙って死なせない) |
| `loop.kill` | `{slug, reason?}` | 意識的に殺す(黙って死なせない) |

慣例: 任意のイベントは `body.loop = slug` を持つことで、そのループへの活動として数えられる。

## 捕捉(1ジェスチャー入力)

| type | body | evidence | 意味 |
|---|---|---|---|
| `video.log` | `{filename, note?}` | 動画ファイルのハッシュ | 朝のビデオ統括。原本はハッシュで参照し、原則コピーしない |
| `video.transcript` | `{video, text}` | 文字起こしテキストのハッシュ | `video` は元イベントの id。AI が書く |
| `note.capture` | `{text, tags?}` | — | アイデア・メモの捕捉。新アイデアは既定で保留場行き |
| `journal.entry` | `{text, filename?, loop?}` | テキストのハッシュ(+任意で原本の音声/動画ハッシュ) | 朝の統括。**媒体非依存 — テキストが本体**。健全性の鐘は `journal.entry` か `video.log` のどちらかを見る |

## 主張と検証(claim ≠ verified)

| type | body | evidence | 意味 |
|---|---|---|---|
| `claim.declare` | `{title, loop?, confidence?}` | **必須** | 完了の主張。`confidence` は記録されるが判定には使われない |
| `claim.verify` | `{claim, verdict, reason?}` | 検証時の証拠 | `claim` は宣言イベントの id。`verdict` は `pass`/`fail`。**宣言と同じ actor は検証できない** |
| `claim.reopen` | `{claim, reason}` | — | 過去の成立を取り消して開き直す |

## 文書の蓄積(地位は宣言せず、導出する)

中核ファイル(理論・方針など)は普通のファイルとして置き、上書きし続けてよい。
台帳に刻むのは「存在」と「残したい版」だけで、**「正典」という地位は書き込み時には存在しない**。
どの文書が中核かは、改訂・参照の履歴から読むときに導出される(棚卸しで番頭に聞く)。

| type | body | evidence | 意味 |
|---|---|---|---|
| `doc.capture` | `{path, note?}` | 文書ファイル(`--store` 推奨) | 文書の存在を記録する。地位は何も与えない。入れる/入れないの決定は不要 |
| `doc.revise` | `{path, version?, note?}` | 改訂後のファイル(`--store` 推奨) | 残したい版を刻む。刻むかどうかは**1回ごと**の選択で、文書ごとの資格審査ではない |

`--store` により、その瞬間のバイト列が `objects/<sha256>` に保全される。ファイル本体を何度
上書きしても、刻んだ全版は内容ハッシュ付きで残る(蔵)。刻まなかった改訂も git には残る。

慣例: 門の前で迷ったこと自体も記録に値する — `banto note "..." --tag mayoi`。

## 予約(v1 では未実装。設計だけ固定)

`receipt.purchase`, `stock.correct` — レシート在庫線。`site.published` — ホームページ線。
これらの参加者は将来この契約を喋る。各製品固有のデータモデルを契約へ押し込まない。
