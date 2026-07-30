#!/bin/sh
# 続編デモの土台: 独立Collector(認証+TLS+権限分離)を立てる
# 前提: root で実行。WORK は台帳や repo の外(証明書と鍵の置き場)
set -eu
WORK="${WORK:-$HOME/otel-vault-work}"
VAULT="${VAULT:-/var/otel-vault}"
VER=0.157.0
DEMO_DIR="$(cd "$(dirname "$0")/.." && pwd)"

mkdir -p "$WORK" && cd "$WORK"

# 1. 本物の Collector(otelcol-contrib)
if [ ! -x otelcol-contrib ]; then
  curl -sL -o otelcol.tar.gz "https://github.com/open-telemetry/opentelemetry-collector-releases/releases/download/v${VER}/otelcol-contrib_${VER}_linux_amd64.tar.gz"
  tar xzf otelcol.tar.gz otelcol-contrib
fi

# 2. TLS 自己署名証明書(デモ用。実運用は正規の PKI を)
if [ ! -f server.crt ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -keyout server.key -out server.crt \
    -days 30 -subj "/CN=localhost" -addext "subjectAltName=DNS:localhost,IP:127.0.0.1"
fi

# 3. Bearer トークン(エージェントは「送る」ことだけ許される)
[ -f token.txt ] || python3 -c "import secrets; print(secrets.token_hex(16))" > token.txt

# 4. 蔵: root 所有 0700 — エージェント権限では読めない・書けない
mkdir -p "$VAULT" && chown root:root "$VAULT" && chmod 700 "$VAULT"

# 5. 非特権のエージェントユーザー
id agentworker >/dev/null 2>&1 || useradd -m agentworker
install -o agentworker -g agentworker -m 600 token.txt /home/agentworker/token.txt
install -o agentworker -g agentworker -m 644 server.crt /home/agentworker/server.crt

# 6. 起動
OTEL_VAULT_TOKEN="$(cat token.txt)" \
OTEL_VAULT_CERT="$WORK/server.crt" \
OTEL_VAULT_KEY="$WORK/server.key" \
OTEL_VAULT_DIR="$VAULT" \
exec ./otelcol-contrib --config "$DEMO_DIR/collector/config.yaml"
