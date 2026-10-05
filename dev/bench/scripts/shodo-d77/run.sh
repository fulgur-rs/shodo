#!/usr/bin/env bash
set -euo pipefail
# Usage: BASE=<baseline d77_scale binary> CAND=<candidate binary> OUT=<samples.jsonl> run.sh
# Runs 12 ABBA rounds, then 12 BAAB rounds; needs jq.
: "${BASE:?path to the baseline d77_scale binary}"
: "${CAND:?path to the candidate d77_scale binary}"
: "${OUT:?output samples.jsonl path}"
: > "$OUT"
# case:size:reps (reps chosen so one baseline sample is roughly 50-250 ms where possible)
CASES="nested:20:10 nested:40:2 nested:80:1 nested:160:1 nestedtab:20:6 nestedtab:40:1 nestedtab:80:1 nestedtab:160:1 siblings:100:1 siblings:200:1 siblings:400:1 ordinary:200:10 ordinary:800:2 plain:200:200 plain:800:50"
for order in ABBA BAAB; do
  if [ "$order" = ABBA ]; then labels="baseline candidate candidate baseline"; else labels="candidate baseline baseline candidate"; fi
  for round in $(seq 1 12); do
    for spec in $CASES; do
      IFS=: read -r case size reps <<< "$spec"
      i=0
      for label in $labels; do
        bin=$BASE; [ "$label" = candidate ] && bin=$CAND
        "$bin" sample "$case" "$size" "$label" "$i" "$reps" \
          | jq -c --arg order "$order" --argjson round "$round" '. + {order: $order, round: $round}' >> "$OUT"
        i=$((i + 1))
      done
    done
  done
done
