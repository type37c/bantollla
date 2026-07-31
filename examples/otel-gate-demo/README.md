# otel-gate-demo — OpenTelemetry トレースを完了ゲートの証拠にする

Zenn 記事「LLMエージェントの「できました」を検証する — OpenTelemetryトレースを
完了ゲートの証拠にする」の証拠一式。記事の主張は「記録があります」ではなく、
ここにある実物で検分できる。

## 構成

| パス | 中身 |
|---|---|
| `agent_task.py` | OTel SDK で計装した作業体(決定的スクリプト。LLM ではない — 記事中で明示) |
| `report.md` | 成果物: crates/ の Rust 行数レポート(13 ファイル・4106 行) |
| `trace.json` | 実行時トレース(単一 trace_id・親子 span・観測属性つき) |
| `ledger/` | **demo 台帳そのもの**(banto、追記専用・SHA-256 ハッシュ連鎖)。claim.declare(agent:claude)→ 別 actor(keisuke)の claim.verify、および本記事自体の公開主張の claim が入っている。自己 verify の拒絶(契約 不変量3)は CLI が弾くため台帳には残らない — 同一 actor で `banto gate verify` を打てば誰でもその場で再現できる |
| `records/` | 記事「想像の話ではありません」節の一次記録(下記) |
| `article/` | 公開した記事本文(= 台帳に宣言された証拠の原本) |

## 再現手順(banto v0.1.0)

```sh
# 0. Python 依存(OTel SDK)を入れる
pip install -r requirements.txt

# 1. 作業体を実行(成果物とトレースが出る)— この example ディレクトリで
python3 agent_task.py

# 2. 検数の独立再計算(記事の「別実装で数え直す」)
#    固定コミットは worktree で開く(働き木を汚さず、examples/ も消えない)
git worktree add /tmp/banto-4c437df 4c437df
find /tmp/banto-4c437df/crates -name '*.rs' | sort | xargs wc -l | tail -1   # → 4106 total
git worktree remove /tmp/banto-4c437df

# 3. 台帳の検分 — この example ディレクトリで
banto verify           # ハッシュ連鎖の健全性
banto log              # declare → verify の実イベント
```

## records/ — 開発史の一次記録

出典は非公開の研究リポジトリ(oscillator-arithmetic)。公開できる範囲を
無改変でエクスポートした。

### 「4/4 ALL PASS」偽完了 2 件(実物)

AI エージェント(Claude)名義のコミットが「ALL PASS」を宣言し、当時の監視は
全緑だったが、後日 `hash()` のプロセスソルト依存(数値非再現)が発覚して
合格条件が僅差 FAIL に覆った:

| commit | 日付 | 宣言(コミットメッセージ) | 後日の訂正 |
|---|---|---|---|
| `8f4008c` | 2026-07-11 | FT-VIEW-1: 観の機械化 — (4/4 ALL PASS) | 条件4 が僅差 FAIL |
| `fa07eb7` | 2026-07-11 | BPU-VS-1: 適応的観選択 — Open ≠ View (4/4 ALL PASS) | H3 の D 条項が僅差 FAIL |
| `ac3f81a` | 2026-07-11 | BPU v5.6 (RT-1 4/4) **+ 再現性バグ修正** | (修正コミット) |

- `real1_phase0_episodes.json` — 開発史 11 エピソードのラベル付き
  グラウンドトゥルース(false_completion 2 / noncanonical 3 / survived 5 /
  censored 1)と再現性プローブの適用結果(ハッシュ起因 3/3 検出・誤警報 0・
  意味的欠陥 2/2 は盲点)

### 60 件 QA と捕捉率 0.081 の定義

- `bpu_real1_phase1b_protocol.md` — 実行前凍結プロトコル(60 問の構成)
- `real1_phase1b_results.json` — 生データ。偽完了 11 件
  (`correct: false` の episode)の自己確信は
  0.85 ×1 / 0.90 ×7 / 0.95 ×2 / 0.98 ×1 — 全件 ≥ 0.85
- `governor_r1_protocol.md` — 政策比較の実行前凍結プロトコル。
  CONF 政策 = 「自己確信 < 0.9 のときだけプローブを走らせる」
- `governor_r1_results.json` / `governor_r1_report.md` — 実測。
  CONF の偽完了捕捉率 0.081(GV4: 確信は統治信号にならない)

## ライセンス

リポジトリ本体と同じ Apache-2.0。関連技術は特許出願済みで、特許ライセンスの
範囲は Apache-2.0 §3 の定めによる。
