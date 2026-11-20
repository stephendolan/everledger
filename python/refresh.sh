#!/bin/bash
set -euo pipefail
DATA_DIR="${EVERLEDGER_DATA_DIR:-$HOME/.everledger}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mkdir -p "$DATA_DIR/raw"
cd "$DATA_DIR/raw"
UA="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"
for p in alchemy blacksmithing enchanting engineering leatherworking tailoring cooking first-aid fishing mining herbalism skinning; do
  python3 "$SCRIPT_DIR/everledger_fetch.py" "${EVERLEDGER_VENDOR_URL:-https://wowforever.ninja}/professions/$p/" "$p.html.tmp" "$UA"
  grep -q "Recipes (" "$p.html.tmp" && mv "$p.html.tmp" "$p.html" || { echo "REFRESH FAILED for $p (page shape changed?)"; exit 1; }
done
python3 "$SCRIPT_DIR/build_catalogue.py"
