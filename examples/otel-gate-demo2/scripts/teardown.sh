#!/bin/sh
# setup.sh が作ったものの後始末。root で実行
set -u
pkill -f otelcol-contrib 2>/dev/null || true
userdel -r agentworker 2>/dev/null || true
rm -rf /var/otel-vault
echo "削除: agentworker ユーザー・/var/otel-vault"
echo "残置: 作業ディレクトリ(既定 \$HOME/otel-vault-work — Collector バイナリと証明書。不要なら手で消す)"
