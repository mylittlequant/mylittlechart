#!/bin/sh
# Browser build into web/pkg: cargo profile `wasm-release` (opt z, lto, 1 CGU, panic=abort,
# stripped) -> wasm-bindgen 0.2.129 -> wasm-opt -Oz, then the sizes (raw and brotli).
# FEATURES="live" adds the data bridge (Binance bars); default is demo candles only.
# Needs: wasm32-unknown-unknown, wasm-bindgen-cli 0.2.129, binaryen wasm-opt (WASM_OPT to
# override), and for the brotli figure python3 with `brotli` (BROTLI_PY to override).
set -e
cd "$(dirname "$0")"
ROOT="$(cd ../.. && pwd)"
T="${CARGO_TARGET_DIR:-$ROOT/target}"
OPT="${WASM_OPT:-wasm-opt}"
PY="${BROTLI_PY:-python3}"
FEAT=""
[ -n "${FEATURES:-}" ] && FEAT="--features $FEATURES"
(cd "$ROOT" && cargo build -p mlc-shell-web2 --lib --target wasm32-unknown-unknown --profile wasm-release $FEAT)
rm -rf web/pkg
wasm-bindgen --target web --no-typescript --out-dir web/pkg "$T/wasm32-unknown-unknown/wasm-release/mlc_shell_web2.wasm"
"$OPT" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals -o web/pkg/mlc_shell_web2_bg.opt.wasm web/pkg/mlc_shell_web2_bg.wasm
mv web/pkg/mlc_shell_web2_bg.opt.wasm web/pkg/mlc_shell_web2_bg.wasm
RAW=$(wc -c < web/pkg/mlc_shell_web2_bg.wasm)
BR=$("$PY" -c "import brotli,sys; print(len(brotli.compress(open(sys.argv[1],'rb').read(), quality=11)))" web/pkg/mlc_shell_web2_bg.wasm)
JS=$(wc -c < web/pkg/mlc_shell_web2.js)
echo "wasm: raw $RAW bytes, brotli $BR bytes (js glue $JS bytes) features=[${FEATURES:-}]"
