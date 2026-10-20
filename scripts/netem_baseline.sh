#!/usr/bin/env bash
# QUIC P2P 互发回路弱网测试基线（roadMap Task 0.6）
#
# 场景：
#   baseline        无损（本机回环）
#   loss5_delay200  5% 丢包 + 200ms 单向延迟
#   loss10_delay200 10% 丢包 + 200ms 单向延迟
#
# 弱网注入方式（自动按平台选择）：
#   macOS:  应用层注入——p2p_bench 的 --loss/--delay 在 QuicSocket 收包路径
#           按包丢弃 + 延迟重注入（免 sudo，跨机器可复现）。
#           说明：本机 macOS 的 dummynet/pf 对回环路径不生效，故不用。
#   Linux:  tc netem（lo 接口根 qdisc，真实网络层注入，需 sudo）
#
# 用法：
#   bash scripts/netem_baseline.sh [rounds] [payload]        # macOS 直接跑
#   sudo bash scripts/netem_baseline.sh [rounds] [payload]   # Linux 需 root
#   bash scripts/netem_baseline.sh --no-impair [rounds] [payload]  # 仅无损基线
#
# 输出：每个场景跑 2 遍验证可复现（±10%），SUMMARY 行写入 docs/baseline_*.log
set -euo pipefail
cd "$(dirname "$0")/.."

NO_IMPAIR=0
if [ "${1:-}" = "--no-impair" ]; then
    NO_IMPAIR=1
    shift
fi
ROUNDS="${1:-100}"
PAYLOAD="${2:-1024}"
OUT="docs/baseline_$(date +%Y%m%d_%H%M%S).log"
mkdir -p docs

OS="$(uname -s)"
run_bench() {  # $@=额外 bench 参数
    cargo run --release --example p2p_bench -p vauid -- \
        --rounds "$ROUNDS" --payload "$PAYLOAD" "$@" 2>/dev/null | grep '^SUMMARY '
}

# ---- 弱网注入 / 清除 --------------------------------------------------------
cleanup_impair() {
    if [ "$OS" = Linux ]; then
        echo "== 清除弱网 (tc netem) =="
        sudo tc qdisc del dev lo root 2>/dev/null || true
    fi
    # macOS 应用层注入随进程结束自动失效，无需清理；
    # 兜底清除历史上 pf/dummynet 方案可能遗留的规则
    if [ "$OS" = Darwin ]; then
        sudo -n pfctl -f /etc/pf.conf >/dev/null 2>&1 || true
        sudo -n pfctl -d >/dev/null 2>&1 || true
        sudo -n dnctl -q flush 2>/dev/null || true
    fi
}

impair_on() {  # $1=delay_ms $2=loss_pct
    [ "$NO_IMPAIR" = 1 ] && return 0
    if [ "$1" = 0 ] && [ "$2" = 0 ]; then
        echo "== baseline：不注入弱网 =="
        return 0
    fi
    if [ "$OS" = Linux ]; then
        echo "== 应用弱网 (tc netem): delay=${1}ms loss=${2}% =="
        sudo tc qdisc replace dev lo root netem delay "${1}ms" loss "${2}%"
    else
        echo "== 应用弱网 (应用层注入): delay=${1}ms(单向) loss=${2}% =="
    fi
}
trap cleanup_impair EXIT

# ---- 场景矩阵 ---------------------------------------------------------------
run_scenario() {  # $1=场景名 $2=delay_ms $3=loss_pct
    echo "======== 场景: $1（rounds=${ROUNDS} payload=${PAYLOAD}） ========" | tee -a "$OUT"
    impair_on "$2" "$3"
    # macOS 用应用层注入传参；Linux 由 tc netem 承担，bench 不传
    local extra=()
    if [ "$OS" != Linux ] && { [ "$2" != 0 ] || [ "$3" != 0 ]; }; then
        extra=(--delay "$2" --loss "$3")
    fi
    for i in 1 2; do
        echo "--- 第 $i 遍 ---" | tee -a "$OUT"
        # bench 失败（如弱网下超时）不中止整个脚本，记录后继续。
        # ${extra[@]+...}：bash 3.2（macOS 自带）set -u 下空数组展开会报
        # unbound variable，必须用此惯用法
        if ! run_bench ${extra[@]+"${extra[@]}"} | tee -a "$OUT"; then
            echo "!!! bench 运行失败（见上方错误），跳过该遍" | tee -a "$OUT"
        fi
    done
}

{
    echo "# vauid 弱网基线 $(date '+%F %T') os=$OS rounds=$ROUNDS payload=$PAYLOAD"
    echo "# SUMMARY 字段: rounds payload frame_bytes handshake_ms rtt_min_ms rtt_avg_ms rtt_p50_ms rtt_p95_ms rtt_max_ms elapsed_ms payload_bytes_per_s wire_bytes_per_s"
    echo "# macOS 应用层注入的 delay 为单向（双端收包路径生效）：delay=200 → RTT 额外约 +400ms"
} > "$OUT"

# 先清一次残留（历史方案可能遗留 pf 规则 / pipe）
cleanup_impair
trap - EXIT

run_scenario baseline       0   0
if [ "$NO_IMPAIR" != 1 ]; then
    run_scenario loss5_delay200  200 5
    run_scenario loss10_delay200 200 10
fi

cleanup_impair
echo ""
echo "基线已写入: $OUT"
[ "$NO_IMPAIR" = 1 ] && echo "（仅无损基线）"
