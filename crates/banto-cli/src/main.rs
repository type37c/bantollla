//! banto CLI — 単一バイナリ。仕様の単一情報源は docs/cli_spec_v1.md。
//!
//! ここは薄い皮に徹する: 引数解釈と出し分けだけを持ち、意味論はカーネル
//! (banto-kernel)と commands.rs に住む。

mod commands;
mod config;
mod render;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "banto",
    version,
    about = "番頭 — 追記専用・ハッシュ連鎖・証拠付きイベント台帳",
    disable_help_subcommand = true
)]
struct Cli {
    /// ワークスペースのルート(省略時は cwd から上へ banto.toml を探す)
    #[arg(long, global = true, value_name = "PATH")]
    root: Option<PathBuf>,

    /// 書き手の上書き(既定は banto.toml の actor、無ければ $USER)
    #[arg(long, global = true, value_name = "NAME")]
    actor: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// ワークスペースを初期化する(banto.toml・空の台帳・ledger/objects/)
    Init,

    /// 汎用イベントを追記する
    Append {
        /// イベントの type(小文字ドット区切り。例: note.capture)
        #[arg(value_name = "TYPE")]
        event_type: String,
        /// JSON オブジェクトの body(既定 {})
        #[arg(long, value_name = "JSON", default_value = "{}")]
        body: String,
        /// 証拠ファイル(複数可)
        #[arg(long, value_name = "FILE")]
        evidence: Vec<PathBuf>,
        /// 冪等キー(同じ op の再送は同一イベントを返す)
        #[arg(long, value_name = "KEY")]
        op: Option<String>,
        /// 証拠を objects/<sha256> へコピーし、uri をそこへ向ける
        #[arg(long)]
        store: bool,
    },

    /// 朝のビデオ統括を記帳する(video.log)
    Video {
        /// 動画ファイル(手元にある場合)
        #[arg(value_name = "FILE", required_unless_present = "sha256")]
        file: Option<PathBuf>,
        /// 手元にない原本の SHA-256(動画は撮った端末に置いたまま記帳する)
        #[arg(
            long,
            value_name = "HEX",
            requires = "filename",
            conflicts_with_all = ["file", "store"]
        )]
        sha256: Option<String>,
        /// --sha256 のときの元ファイル名
        #[arg(long, value_name = "NAME")]
        filename: Option<String>,
        /// --sha256 のときのバイト数(任意)
        #[arg(long, value_name = "N")]
        bytes: Option<u64>,
        /// ひとことメモ
        #[arg(long, value_name = "TEXT")]
        note: Option<String>,
        /// このループへの活動として数える
        #[arg(long = "loop", value_name = "SLUG")]
        loop_slug: Option<String>,
        /// 動画を objects/ へコピーする(動画は大きいので原則使わない)
        #[arg(long)]
        store: bool,
    },

    /// 朝の統括を記帳する(journal.entry。媒体非依存 — テキストが本体)
    Journal {
        /// 統括のテキスト(文字起こしなど)
        #[arg(
            long,
            value_name = "TEXT",
            conflicts_with = "file",
            required_unless_present = "file"
        )]
        text: Option<String>,
        /// 統括テキストのファイル
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,
        /// 原本(音声/動画)の SHA-256(任意。端末で `shasum -a 256` )
        #[arg(long, value_name = "HEX", requires = "filename")]
        sha256: Option<String>,
        /// 原本のファイル名(--sha256 とセット)
        #[arg(long, value_name = "NAME")]
        filename: Option<String>,
        /// 原本のバイト数(任意)
        #[arg(long, value_name = "N")]
        bytes: Option<u64>,
        /// このループへの活動として数える
        #[arg(long = "loop", value_name = "SLUG")]
        loop_slug: Option<String>,
    },

    /// 文字起こしを記帳する(video.transcript)
    Transcript {
        /// 元になる video.log イベントの id
        #[arg(value_name = "EVENT_ID")]
        event_id: String,
        /// 文字起こしテキストのファイル
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with = "text",
            required_unless_present = "text"
        )]
        file: Option<PathBuf>,
        /// 文字起こしテキストそのもの
        #[arg(long, value_name = "TEXT")]
        text: Option<String>,
    },

    /// アイデア・メモを捕捉する(note.capture。新アイデアは既定で保留場行き)
    Note {
        #[arg(value_name = "TEXT")]
        text: String,
        /// タグ(複数可)
        #[arg(long = "tag", value_name = "TAG")]
        tags: Vec<String>,
    },

    /// ループ(番頭の領分)
    Loop {
        #[command(subcommand)]
        cmd: LoopCmd,
    },

    /// 朝礼ビュー(稼働中ループを放置日数の降順で差し出す)
    Brief {
        /// 機械可読 JSON を出す(AI 執務層向け)
        #[arg(long)]
        json: bool,
    },

    /// 台帳の健全性(鐘が鳴っていれば終了コード 1)
    Health {
        /// 機械可読 JSON を出す(AI 執務層向け)
        #[arg(long)]
        json: bool,
    },

    /// イベント一覧(時系列、既定は直近20件)
    Log {
        /// type の前方一致で絞り込む
        #[arg(long = "type", value_name = "PREFIX")]
        type_prefix: Option<String>,
        /// 表示件数
        #[arg(long, value_name = "N", default_value_t = 20)]
        limit: usize,
        /// 機械可読 JSON を出す(AI 執務層向け)
        #[arg(long)]
        json: bool,
    },

    /// 台帳全検査(問題があれば一覧して終了コード 1)
    Verify {
        /// 証拠ファイルの存在も確かめる
        #[arg(long)]
        evidence: bool,
        /// 証拠ファイルのハッシュ再計算まで行う
        #[arg(long)]
        deep: bool,
    },

    /// ゲート(claim ≠ verified)
    Gate {
        #[command(subcommand)]
        cmd: GateCmd,
    },
}

#[derive(Subcommand)]
enum LoopCmd {
    /// ループを開く(同じ slug が稼働中ならエラー)
    Open {
        #[arg(value_name = "SLUG")]
        slug: String,
        /// ループの題
        #[arg(long, value_name = "T")]
        title: String,
        /// 種別(既定 project)
        #[arg(long, value_parser = ["project", "habit"])]
        kind: Option<String>,
        /// 次の一手(15〜30分で終わる粒度にする)
        #[arg(long, value_name = "ACTION")]
        next: Option<String>,
        /// 閉じた・保留・殺したループを開き直す
        #[arg(long)]
        reopen: bool,
    },
    /// 次の一手を差し替える(15〜30分で終わる粒度にする)
    Next {
        #[arg(value_name = "SLUG")]
        slug: String,
        /// 新しい次の一手(15〜30分で終わる大きさに刻む)
        #[arg(value_name = "ACTION")]
        action: String,
    },
    /// 進捗を記録する
    Touch {
        #[arg(value_name = "SLUG")]
        slug: String,
        #[arg(long, value_name = "TEXT")]
        note: Option<String>,
    },
    /// 完了して閉じる(番頭の存在意義)
    Close {
        #[arg(value_name = "SLUG")]
        slug: String,
        #[arg(long, value_name = "TEXT")]
        outcome: Option<String>,
    },
    /// 意識的に保留場へ送る(黙って死なせない)
    Park {
        #[arg(value_name = "SLUG")]
        slug: String,
        #[arg(long, value_name = "TEXT")]
        reason: Option<String>,
    },
    /// 意識的に殺す(黙って死なせない)
    Kill {
        #[arg(value_name = "SLUG")]
        slug: String,
        #[arg(long, value_name = "TEXT")]
        reason: Option<String>,
    },
    /// ループ一覧(既定は active のみ)
    List {
        /// 全状態を表示する
        #[arg(long)]
        all: bool,
        /// 機械可読 JSON を出す(AI 執務層向け)
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum GateCmd {
    /// 完了を主張する(claim.declare。証拠が0個ならエラー — 契約 不変量2)
    Declare {
        #[arg(value_name = "TITLE")]
        title: String,
        /// 関係するループ
        #[arg(long = "loop", value_name = "SLUG")]
        loop_slug: Option<String>,
        /// 確信度(記録されるが判定には使われない — 契約 G-2)
        #[arg(long, value_name = "F")]
        confidence: Option<f64>,
        /// 一次証拠のファイル(必須、複数可)
        #[arg(long, value_name = "FILE")]
        evidence: Vec<PathBuf>,
        /// 証拠を objects/<sha256> へコピーする
        #[arg(long)]
        store: bool,
    },
    /// 宣言の証拠を deep 検査し、検証状況を表示する
    Check {
        #[arg(value_name = "CLAIM_ID")]
        claim_id: String,
    },
    /// 独立した actor が検証する(宣言と同じ actor はエラー — 契約 不変量3)
    Verify {
        #[arg(value_name = "CLAIM_ID")]
        claim_id: String,
        /// 判定
        #[arg(long, value_parser = ["pass", "fail"])]
        verdict: String,
        #[arg(long, value_name = "TEXT")]
        reason: Option<String>,
        /// 検証時の証拠(複数可)
        #[arg(long, value_name = "FILE")]
        evidence: Vec<PathBuf>,
    },
    /// 過去の成立を取り消して開き直す(claim.reopen)
    Reopen {
        #[arg(value_name = "CLAIM_ID")]
        claim_id: String,
        #[arg(long, value_name = "TEXT")]
        reason: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let code = match run(cli) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("banto: {msg}");
            1
        }
    };
    std::process::exit(code);
}

fn run(cli: Cli) -> Result<i32, String> {
    if matches!(cli.command, Command::Init) {
        let root = match &cli.root {
            Some(r) => r.clone(),
            None => std::env::current_dir().map_err(|e| format!("cwd を取得できない: {e}"))?,
        };
        return commands::cmd_init(&root, cli.actor.as_deref());
    }
    let ws = config::discover(cli.root.as_deref(), cli.actor.as_deref())?;
    match cli.command {
        Command::Init => unreachable!("init は上で処理済み"),
        Command::Append {
            event_type,
            body,
            evidence,
            op,
            store,
        } => commands::cmd_append(&ws, &event_type, &body, &evidence, op, store),
        Command::Video {
            file,
            sha256,
            filename,
            bytes,
            note,
            loop_slug,
            store,
        } => match (file, sha256) {
            (Some(file), None) => commands::cmd_video(&ws, &file, note, loop_slug, store),
            (None, Some(sha256)) => commands::cmd_video_remote(
                &ws,
                &sha256,
                &filename.expect("clap requires filename with sha256"),
                bytes,
                note,
                loop_slug,
            ),
            _ => unreachable!("clap enforces exactly one of FILE / --sha256"),
        },
        Command::Journal {
            text,
            file,
            sha256,
            filename,
            bytes,
            loop_slug,
        } => commands::cmd_journal(
            &ws,
            text,
            file.as_deref(),
            sha256,
            filename,
            bytes,
            loop_slug,
        ),
        Command::Transcript {
            event_id,
            file,
            text,
        } => commands::cmd_transcript(&ws, &event_id, file.as_deref(), text),
        Command::Note { text, tags } => commands::cmd_note(&ws, text, tags),
        Command::Loop { cmd } => match cmd {
            LoopCmd::Open {
                slug,
                title,
                kind,
                next,
                reopen,
            } => commands::cmd_loop_open(&ws, slug, title, kind, next, reopen),
            LoopCmd::Next { slug, action } => {
                commands::cmd_loop_verb(&ws, "next", slug, "action", Some(action))
            }
            LoopCmd::Touch { slug, note } => {
                commands::cmd_loop_verb(&ws, "touch", slug, "note", note)
            }
            LoopCmd::Close { slug, outcome } => {
                commands::cmd_loop_verb(&ws, "close", slug, "outcome", outcome)
            }
            LoopCmd::Park { slug, reason } => {
                commands::cmd_loop_verb(&ws, "park", slug, "reason", reason)
            }
            LoopCmd::Kill { slug, reason } => {
                commands::cmd_loop_verb(&ws, "kill", slug, "reason", reason)
            }
            LoopCmd::List { all, json } => commands::cmd_loop_list(&ws, all, json),
        },
        Command::Brief { json } => commands::cmd_brief(&ws, json),
        Command::Health { json } => commands::cmd_health(&ws, json),
        Command::Log {
            type_prefix,
            limit,
            json,
        } => commands::cmd_log(&ws, type_prefix, limit, json),
        Command::Verify { evidence, deep } => commands::cmd_verify(&ws, evidence, deep),
        Command::Gate { cmd } => match cmd {
            GateCmd::Declare {
                title,
                loop_slug,
                confidence,
                evidence,
                store,
            } => commands::cmd_gate_declare(&ws, title, loop_slug, confidence, &evidence, store),
            GateCmd::Check { claim_id } => commands::cmd_gate_check(&ws, &claim_id),
            GateCmd::Verify {
                claim_id,
                verdict,
                reason,
                evidence,
            } => commands::cmd_gate_verify(&ws, &claim_id, verdict, reason, &evidence),
            GateCmd::Reopen { claim_id, reason } => {
                commands::cmd_gate_reopen(&ws, &claim_id, reason)
            }
        },
    }
}
