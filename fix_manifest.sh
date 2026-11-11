#!/usr/bin/env bash
set -euo pipefail
M=progress/manifest.tsv
[ -f "$M" ] || { echo "ERROR: $M not found" >&2; exit 1; }
[ -e progress.bak ] || cp -r progress progress.bak
awk -F'\t' -v OFS='\t' '
NR==1 { print; next }
$7 ~ /vanilla file, not Create helper/ { print "REMOVED: " $1 > "/dev/stderr"; next }
NF==7 && $1 ~ /^hook_stub_/ { $8 = "hook_stub"; print "KIND ADDED: " $1 > "/dev/stderr" }
$1 ~ /^android_bundled_decl_/ { $8 = "out_of_scope" }
$7 ~ /data missing/ && ($5 == "DONE_UNVERIFIED" || $5 == "VERIFIED") { $5 = "TODO" }
{ print }
' "$M" > "$M.new"
mv "$M.new" "$M"
echo "--- columns per row (expect only 8):"; awk -F'\t' '{print NF}' "$M" | sort | uniq -c
echo "--- ends with newline? (expect 0a):"; tail -c 1 "$M" | xxd -p
echo "--- kind x status:"; tail -n +2 "$M" | awk -F'\t' '{print $8"\t"$5}' | sort | uniq -c
