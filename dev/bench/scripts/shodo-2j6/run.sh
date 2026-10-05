#!/usr/bin/env bash
set -euo pipefail
# Usage: BASE=<baseline sibling_scale binary> CAND=<candidate binary> OUT=<samples.jsonl> run.sh
# Runs 12 ABBA rounds, then 12 BAAB rounds; needs jq. Takes about 1.5-2 hours
# (the baseline's siblings 1600 sample alone is about 45 s).
: "${BASE:?path to the baseline sibling_scale binary}"
: "${CAND:?path to the candidate sibling_scale binary}"
: "${OUT:?output samples.jsonl path}"
: > "$OUT"
# case:size:reps
CASES="siblings:200:1 siblings:400:1 siblings:800:1 siblings:1600:1 outer:100:1 outer:200:1 outer:400:1 outer:800:1 valign:200:1 valign:400:1 valign:800:1 rtl:200:1 rtl:400:1 rtl:800:1 ordinary:200:10 ordinary:800:2 plain:200:200 plain:800:50"
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
