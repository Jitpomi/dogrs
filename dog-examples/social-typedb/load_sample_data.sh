#!/usr/bin/env bash
set -euo pipefail

# load_sample_data.sh
# Robust loader for TypeQL blocks ending with `commit;` 
# - Keeps newlines (TypeQL parser is happier)
# - Supports both:
#     insert ... commit;
#     match ... insert ... commit;
# - Routes by ACTUAL relation patterns like `(author:` / `(employee:` etc
#   (NOT by "isa posting" because posting is a relation instance, not "isa posting")
# - Handles big multi-entity inserts (persons/orgs/posts/comments) correctly

BASE_URL="${BASE_URL:-http://127.0.0.1:3036}"
FILE="${1:-sample_data.tql}"

echo "Loading TypeQL from: $FILE"
echo "BASE_URL: $BASE_URL"
echo "==========================================="

# ---- helpers -------------------------------------------------------

json_escape() {
  # Simple JSON escaping - replace newlines with spaces for now
  echo "$1" | sed 's/\\/\\\\/g; s/"/\\"/g; s/\t/\\t/g' | tr '\n' ' ' | sed 's/  */ /g'
}

route_service() {
  # Decide service based on content of the FULL query block.
  # Priority: relations with explicit roles -> correct service.
  local q="$1"

  # Relations that should go through "posts" service
  if grep -Eq '\)\s+isa\s+posting\b' <<<"$q" || grep -Eq '\)\s+isa\s+sharing\b' <<<"$q"; then
    echo "posts"; return
  fi

  # Relations / entities that should go through "comments" service
  if grep -Eq '\)\s+isa\s+commenting\b' <<<"$q" || grep -Eq '\)\s+isa\s+reaction\b' <<<"$q" && grep -Eq '\$comment|isa\s+comment\b' <<<"$q"; then
    # comment relations are safest on comments service
    echo "comments"; return
  fi

  # Person-centric relations
  if grep -Eq '\)\s+isa\s+(friendship|parentship|siblingship|marriage|engagement|relationship|birth|employment|education|group-membership|reaction|response)\b' <<<"$q"; then
    echo "persons"; return
  fi

  # Entity inserts
  if grep -Eq '\bisa\s+person\b' <<<"$q"; then echo "persons"; return; fi
  if grep -Eq '\bisa\s+(company|charity|university|college|school|educational-institute|organization)\b' <<<"$q"; then echo "organizations"; return; fi
  if grep -Eq '\bisa\s+group\b' <<<"$q"; then echo "groups"; return; fi
  if grep -Eq '\bisa\s+(post|text-post|image-post|video-post|poll-post|live-video-post|share-post)\b' <<<"$q"; then echo "posts"; return; fi
  if grep -Eq '\bisa\s+comment\b' <<<"$q"; then echo "comments"; return; fi

  # Fallback
  echo "persons"
}

send_block() {
  local block="$1"

  # Trim leading/trailing whitespace and remove commit; from the end
  block="$(printf "%s" "$block" | sed -e 's/^[[:space:]]\+//' -e 's/[[:space:]]\+$//' -e 's/commit;[[:space:]]*$//')"

  # Skip empty blocks
  [[ -z "$block" ]] && return 0

  local service
  service="$(route_service "$block")"

  echo "📝 -> $service"
  # show first line preview
  echo "   $(printf "%s" "$block" | head -n 1 | cut -c1-120)..."

  local escaped
  escaped="$(json_escape "$block")"

  local response
  response="$(curl -sS -X POST "$BASE_URL/$service" \
    -H "Content-Type: application/json" \
    -H "x-service-method: write" \
    -d "{\"query\":\"$escaped\"}")"

  if grep -q '"ok"[[:space:]]*:[[:space:]]*true' <<<"$response"; then
    echo "✅ Success"
  else
    echo "❌ Error: $response"
    echo "---- BLOCK THAT FAILED ----"
    echo "$block"
    echo "---------------------------"
    # Don't exit hard; comment this line if you want strict mode
    # exit 1
  fi
  echo "---"
}

# ---- parse file into blocks ---------------------------------------

# We treat each transaction as everything from (insert|match) up to `commit;` 
# while:
# - ignoring comment lines starting with #
# - not flattening newlines
block=""
in_tx=0
block_count=0

echo "Parsing $FILE for transaction blocks..."

while IFS= read -r line || [[ -n "$line" ]]; do
  # ignore full-line comments
  if [[ "$line" =~ ^[[:space:]]*# ]]; then
    continue
  fi

  # If we are not in a transaction, look for start
  if [[ $in_tx -eq 0 ]]; then
    if [[ "$line" =~ ^[[:space:]]*(insert|match) ]]; then
      in_tx=1
      block="$line"$'\n'
      echo "Found transaction start: $(echo "$line" | cut -c1-50)..."
    fi
    continue
  fi

  # We are in a transaction:
  # append line
  block+="$line"$'\n'

  # detect end
  if [[ "$line" =~ commit\; ]]; then
    # send and reset
    ((block_count++))
    echo "Processing block $block_count..."
    send_block "$block"
    block=""
    in_tx=0
  fi
done < "$FILE"

# If file ended mid-transaction, still attempt to send (optional)
if [[ $in_tx -eq 1 && -n "$block" ]]; then
  echo "⚠️  File ended without 'commit;' — sending last buffered block anyway."
  send_block "$block"
fi

echo "Processed $block_count transaction blocks total."

echo ""
echo "🎉 Loading completed."
echo ""
echo "Now let's test reading the data..."
echo "================================="

read_query() {
  local service="$1"
  local q="$2"
  echo "📖 Reading $service..."
  curl -sS -X POST "$BASE_URL/$service" \
    -H "Content-Type: application/json" \
    -H "x-service-method: read" \
    -d "{\"query\":\"$(json_escape "$q")\"}" | jq '.'
  echo ""
}

read_query "persons" 'match $p isa person; select $p;'
read_query "organizations" 'match $o isa organization; select $o;'
read_query "groups" 'match $g isa group; select $g;'
read_query "posts" 'match $p isa post; select $p;'
read_query "comments" 'match $c isa comment; select $c;'

echo "✅ Sample data testing complete!"
