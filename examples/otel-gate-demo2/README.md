# otel-gate-demo2 — AI が記録を改竄できない構造を、OpenTelemetry Collector で作る

Zenn 記事「LLMエージェントの「できました」を検証する(2)」の証拠一式。
前作([otel-gate-demo](../otel-gate-demo/))が白状した二つの弱点 —
宣言者自身がトレースを吐いていた/偽完了の実物が出てこなかった — を、
標準部品だけで塞いだ実録が入っている。

## 構成

| パス | 中身 |
|---|---|
| `collector/config.yaml` | 独立 Collector の設定30行(OTLP/HTTPS 受信+TLS+`bearertokenauth`+file exporter)。otelcol-contrib v0.157.0 の標準部品のみ |
| `scripts/setup.sh` | 再現用: Collector 取得・証明書生成・トークン・蔵(root所有0700)・非特権ユーザー作成・起動 |
| `agent_task_otlp.py` | 正直な作業体。前作と同じ行数集計、exporter の向き先だけ境界の向こうへ |
| `tamper_resend.py` | 第二幕の道具: 「都合よく直した」トレースの再送 → 上書きにならず追記になる |
| `agent_task_lazy.py` | 第四幕の作業体: 作業せず span とレポートを捏造(lines.total: 5000) |
| `transcript.md` | **四幕+終幕の全実録**(401 / Permission denied / 差し替えの追記化 / 自己verify拒絶 / 捏造→再計算→fail) |
| `report.md` / `report_lazy.md` | 成果物(正直版 4106 / 捏造版 5000) |
| `vault_export.jsonl` | 蔵の受領記録スナップショット(正直 4106・再送 9999・捏造 5000 の三 trace) |
| `ledger/` | **demo 台帳そのもの**。claim.declare ×2(正直・怠惰)→ keisuke の claim.verify **pass / fail**。偽完了が門で殺された記録が公開のまま残っている |
| `article/` | 公開した記事本文(台帳に宣言された証拠の原本) |

## 役割分担(この分離が本体)

- **作業体** — 非特権ユーザー `agentworker`(actor: `agent:claude`)。送る資格(Bearer トークン)は持つが、蔵には読み書きできない
- **境界の運用者** — root。Collector と蔵 `/var/otel-vault`(0700)を保持
- **検証者** — `keisuke`。宣言とは別の actor。蔵の記録・成果物・固定した木への再計算を突き合わせて判定する

## 再現手順

前提: **Linux/amd64 専用**。setup.sh はホストを改変する — システムユーザー
`agentworker` と `/var/otel-vault`(root 所有 0700)を恒久作成する。
後始末は `scripts/teardown.sh`。

```sh
# 0. Python 依存を入れる
pip install -r requirements.txt

# 1. Collector 起動(別端末で。上記のホスト改変に同意の上で)
sudo sh scripts/setup.sh

# 2. 作業体として実行(setup.sh が agentworker の手元に置いた鍵とトークンを渡す)
sudo -u agentworker env \
  OTLP_CA_CERT=/home/agentworker/server.crt \
  OTLP_TOKEN="$(sudo cat /home/agentworker/token.txt)" \
  python3 agent_task_otlp.py <repo> report.md
#  (Python 依存は実行ユーザーから見える場所に。venv なら venv の python3 を指す)

# 3. 台帳の検分 — この example ディレクトリで
banto verify && banto log

# 4. 検数の独立再計算(固定コミットは worktree で開く。働き木を汚さない)
git worktree add /tmp/banto-4c437df 4c437df
find /tmp/banto-4c437df/crates -name '*.rs' | xargs wc -l | tail -1   # → 4106 total
git worktree remove /tmp/banto-4c437df
```

## ライセンス

リポジトリ本体と同じ Apache-2.0。関連技術は特許出願済みで、特許ライセンスの
範囲は Apache-2.0 §3 の定めによる。
