#!/usr/bin/env bash
set -euo pipefail
# Usage: BASE=<baseline tab_scale binary> CAND=<candidate binary> OUT=<samples.jsonl> run.sh
# Runs 12 ABBA rounds, then 12 BAAB rounds; needs jq. Takes about 30-60 minutes
# (the baseline's nestedtab 160 and siblingstab 800 samples dominate).
: "${BASE:?path to the baseline tab_scale binary}"
: "${CAND:?path to the candidate tab_scale binary}"
: "${OUT:?output samples.jsonl path}"
: > "$OUT"
# case:size:reps
CASES="nested:20:4 nested:40:2 nested:80:1 nested:160:1 nestedtab:20:4 nestedtab:40:2 nestedtab:80:1 nestedtab:160:1 siblings:200:1 siblings:400:1 siblings:800:1 siblingstab:200:1 siblingstab:400:1 siblingstab:800:1 plain:200:50 plain:800:10"
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
