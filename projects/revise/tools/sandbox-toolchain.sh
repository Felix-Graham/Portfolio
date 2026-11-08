#!/usr/bin/env bash
# Sets up a working Rust toolchain in a Claude sandbox (Ubuntu 24.04, no
# rustup access) so `cargo test` / `cargo build` can be run for real.
#
# Why this exists: Cargo.toml's egui 0.36.2 needs Rust 1.95, but the
# sandbox can only apt-install up to rustc-1.91. Rust 1.91 compiles the
# project fine once four small uses of then-unstable std APIs inside the
# *dependency* sources are rewritten. This patches a scratch copy under
# $CARGO_HOME only; nothing in the project source is touched, and none
# of this is needed on a normal machine with a current toolchain.
#
# Usage:   bash tools/sandbox-toolchain.sh
# Then:    export CARGO_HOME=/home/claude/.cargo
#          cargo-1.91 test --lib --ignore-rust-version
#          cargo-1.91 build --bins --ignore-rust-version
# (a cold build takes a few minutes; run it in the background with
#  nohup and poll the log if the tool call time limit is short).
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="${CARGO_HOME:-/home/claude/.cargo}"

if ! command -v cargo-1.91 >/dev/null 2>&1; then
  apt-get install -y rustc-1.91 cargo-1.91
fi

cargo-1.91 fetch --locked 2>/dev/null || cargo-1.91 fetch

python3 - <<'PY'
import glob, os
home = os.environ["CARGO_HOME"]

def patch(pattern, old, new, label):
    paths = glob.glob(f"{home}/registry/src/*/{pattern}")
    if not paths:
        print(f"  {label}: source not found (version changed?) — skipped")
        return
    for p in paths:
        s = open(p).read()
        if old in s:
            open(p, "w").write(s.replace(old, new))
            print(f"  {label}: patched")
        else:
            print(f"  {label}: already patched or different version")

patch("emath-0.36.2/src/history.rs",
"""            && self
                .values
                .pop_front_if(|&mut (front_time, _)| front_time < oldest_allowed_time)
                .is_some()
        {""",
"""            && self.values.front().is_some_and(|&(front_time, _)| front_time < oldest_allowed_time)
            && self.values.pop_front().is_some()
        {""", "emath history.rs (VecDeque::pop_front_if)")

patch("epaint-0.36.2/src/shapes/shape.rs",
"for &[start, end] in path.array_windows() {",
"for w in path.windows(2) {\n        let (start, end) = (w[0], w[1]);",
"epaint shape.rs (slice::array_windows)")

patch("epaint-0.36.2/src/text/text_layout.rs",
"paragraph = out_paragraphs.push_mut(Paragraph::from_section_index(section_index));",
"out_paragraphs.push(Paragraph::from_section_index(section_index));\n            paragraph = out_paragraphs.last_mut().unwrap();",
"epaint text_layout.rs (Vec::push_mut)")

patch("egui-0.36.2/src/util/undoer.rs",
"if let Some(state) = self.undos.pop_back_if(|state| &*state == current_state) {",
"if let Some(state) = (if self.undos.back().is_some_and(|state| state == current_state) { self.undos.pop_back() } else { None }) {",
"egui undoer.rs (VecDeque::pop_back_if)")
PY

echo
echo "Done. Next:"
echo "  export CARGO_HOME=$CARGO_HOME"
echo "  cargo-1.91 test --lib --ignore-rust-version"
