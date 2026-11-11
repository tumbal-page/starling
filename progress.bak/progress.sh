#!/usr/bin/env bash
# Progress tracker for Create Android bundled port
# Reads manifest.tsv and prints counts per status, total, and percentage

set -euo pipefail

MANIFEST="/root/proxd/starling/progress/manifest.tsv"

if [[ ! -f "$MANIFEST" ]]; then
    echo "ERROR: manifest not found at $MANIFEST" >&2
    exit 1
fi

# Count total rows (excluding header)
TOTAL=$(tail -n +2 "$MANIFEST" | wc -l)

# Count per status
TODO=$(tail -n +2 "$MANIFEST" | awk -F'\t' '$5 == "TODO" {count++} END {print count+0}')
IN_PROGRESS=$(tail -n +2 "$MANIFEST" | awk -F'\t' '$5 == "IN_PROGRESS" {count++} END {print count+0}')
DONE_UNVERIFIED=$(tail -n +2 "$MANIFEST" | awk -F'\t' '$5 == "DONE_UNVERIFIED" {count++} END {print count+0}')
VERIFIED=$(tail -n +2 "$MANIFEST" | awk -F'\t' '$5 == "VERIFIED" {count++} END {print count+0}')
BLOCKED=$(tail -n +2 "$MANIFEST" | awk -F'\t' '$5 == "BLOCKED" {count++} END {print count+0}')

# Verify DONE_UNVERIFIED: target_file exists and non-empty
VERIFIED_DONE_UNVERIFIED=0
while IFS=$'\t' read -r id toml_entry source_file target_file status evidence notes kind; do
    if [[ "$status" == "DONE_UNVERIFIED" ]]; then
        if [[ -f "/root/proxd/starling/crates/horizon-client/src/game/android_bundled/create/$target_file" ]] && [[ -s "/root/proxd/starling/crates/horizon-client/src/game/android_bundled/create/$target_file" ]]; then
            ((VERIFIED_DONE_UNVERIFIED++))
        fi
    fi
done < <(tail -n +2 "$MANIFEST") || true

# Count per kind
KINDS=$(tail -n +2 "$MANIFEST" | awk -F'\t' '{print $8}' | sort -u)
echo "=== Progress Report ==="
echo "Total rows: $TOTAL"
echo ""

# Overall status counts
echo "Overall status counts:"
echo "  TODO:             $TODO"
echo "  IN_PROGRESS:      $IN_PROGRESS"
echo "  DONE_UNVERIFIED:  $DONE_UNVERIFIED (file exists & non-empty: $VERIFIED_DONE_UNVERIFIED)"
echo "  VERIFIED:         $VERIFIED"
echo "  BLOCKED:          $BLOCKED"
echo ""

# Overall percentages
PCT_VERIFIED=0
PCT_DONE_PLUS_VERIFIED=0
if [[ $TOTAL -gt 0 ]]; then
    PCT_VERIFIED=$(awk "BEGIN { printf \"%.1f\", ($VERIFIED / $TOTAL) * 100 }")
    PCT_DONE_PLUS_VERIFIED=$(awk "BEGIN { printf \"%.1f\", (($VERIFIED + $VERIFIED_DONE_UNVERIFIED) / $TOTAL) * 100 }")
fi
echo "Overall percentages:"
echo "  VERIFIED / Total:                    $VERIFIED / $TOTAL = ${PCT_VERIFIED}%"
echo "  (VERIFIED + DONE_UNVERIFIED) / Total: $((VERIFIED + VERIFIED_DONE_UNVERIFIED)) / $TOTAL = ${PCT_DONE_PLUS_VERIFIED}%"
echo ""

# Per-kind breakdown
echo "=== Per-kind breakdown ==="
for kind in $KINDS; do
    K_TOTAL=$(tail -n +2 "$MANIFEST" | awk -F'\t' -v k="$kind" '$8 == k {count++} END {print count+0}')
    K_TODO=$(tail -n +2 "$MANIFEST" | awk -F'\t' -v k="$kind" '$8 == k && $5 == "TODO" {count++} END {print count+0}')
    K_IN_PROGRESS=$(tail -n +2 "$MANIFEST" | awk -F'\t' -v k="$kind" '$8 == k && $5 == "IN_PROGRESS" {count++} END {print count+0}')
    K_DONE_UNVERIFIED=$(tail -n +2 "$MANIFEST" | awk -F'\t' -v k="$kind" '$8 == k && $5 == "DONE_UNVERIFIED" {count++} END {print count+0}')
    K_VERIFIED=$(tail -n +2 "$MANIFEST" | awk -F'\t' -v k="$kind" '$8 == k && $5 == "VERIFIED" {count++} END {print count+0}')
    K_BLOCKED=$(tail -n +2 "$MANIFEST" | awk -F'\t' -v k="$kind" '$8 == k && $5 == "BLOCKED" {count++} END {print count+0}')

    K_VERIFIED_DONE=0
    while IFS=$'\t' read -r id toml_entry source_file target_file status evidence notes k; do
        if [[ "$k" == "$kind" && "$status" == "DONE_UNVERIFIED" ]]; then
            if [[ -f "/root/proxd/starling/crates/horizon-client/src/game/android_bundled/create/$target_file" ]] && [[ -s "/root/proxd/starling/crates/horizon-client/src/game/android_bundled/create/$target_file" ]]; then
                ((K_VERIFIED_DONE++))
            fi
        fi
    done < <(tail -n +2 "$MANIFEST") || true

    K_PCT_VERIFIED=0
    K_PCT_DONE_PLUS=0
    if [[ $K_TOTAL -gt 0 ]]; then
        K_PCT_VERIFIED=$(awk "BEGIN { printf \"%.1f\", ($K_VERIFIED / $K_TOTAL) * 100 }")
        K_PCT_DONE_PLUS=$(awk "BEGIN { printf \"%.1f\", (($K_VERIFIED + $K_VERIFIED_DONE) / $K_TOTAL) * 100 }")
    fi

    echo "Kind: $kind ($K_TOTAL rows)"
    echo "  TODO:             $K_TODO"
    echo "  IN_PROGRESS:      $K_IN_PROGRESS"
    echo "  DONE_UNVERIFIED:  $K_DONE_UNVERIFIED (file exists & non-empty: $K_VERIFIED_DONE)"
    echo "  VERIFIED:         $K_VERIFIED"
    echo "  BLOCKED:          $K_BLOCKED"
    echo "  VERIFIED / Total:                    $K_VERIFIED / $K_TOTAL = ${K_PCT_VERIFIED}%"
    echo "  (VERIFIED + DONE_UNVERIFIED) / Total: $((K_VERIFIED + K_VERIFIED_DONE)) / $K_TOTAL = ${K_PCT_DONE_PLUS}%"
    echo ""
done