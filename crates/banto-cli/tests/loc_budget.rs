//! 肥大への構造的歯止め: CLI の src/ は 3500 行を超えてはならない。
//!
//! bgit は「インフラの精緻さが研究の実速度を上回った」ことで死んだ。
//! この上限を上げる変更は、それ自体を台帳に claim.declare として刻むこと。

use std::path::Path;

const BUDGET: usize = 3500;

fn count_lines(dir: &Path) -> usize {
    let mut total = 0;
    for entry in std::fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            total += count_lines(&path);
        } else if path.extension().is_some_and(|e| e == "rs") {
            total += std::fs::read_to_string(&path)
                .expect("read source file")
                .lines()
                .count();
        }
    }
    total
}

#[test]
fn cli_stays_within_loc_budget() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let lines = count_lines(&src);
    assert!(
        lines <= BUDGET,
        "banto-cli src/ is {lines} lines, budget is {BUDGET}. \
         Do not raise the budget casually — bgit died of ceremony bloat."
    );
}
