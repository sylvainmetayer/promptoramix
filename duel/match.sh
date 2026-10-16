#!/usr/bin/env bash
# Match parallèle contre la maison : ./duel/match.sh <shards> [config]  (LINKX_CONFIG pour notre IA)
cd "$(dirname "$0")"
n=${1:-3}
export LINKX_CONFIG=${2:-}
for i in $(seq 0 $((n - 1))); do
  mise exec node@24 -- node duel.ts --games 34 --openings --ours-ms 4500 --house-ms 900 --shard "$i/$n" 2>/dev/null | grep '^#' &
done
wait
