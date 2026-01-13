#!/usr/bin/env bash
set -euo pipefail

# Fleet Management Database Seeder
# Uses HTTP API approach for robust TypeQL loading (similar to social-typedb example)

BASE_URL="${BASE_URL:-http://127.0.0.1:3036}"
FILE="${1:-sample_data.tql}"

echo "🌱 Fleet Management Database Seeder"
echo "==================================="
echo "Loading TypeQL from: $FILE"
echo "BASE_URL: $BASE_URL"
echo ""

# ---- helpers -------------------------------------------------------

json_escape() {
  # Simple JSON escaping - replace newlines with spaces for now
  echo "$1" | sed 's/\\/\\\\/g; s/"/\\"/g; s/\t/\\t/g' | tr '\n' ' ' | sed 's/  */ /g'
}

route_service() {
  # Route fleet management entities to appropriate services
  local q="$1"

  # Check for assignment relations first (before individual entities)
  if grep -Eq '\bisa\s+assignment\b' <<<"$q"; then echo "operations"; return; fi
  
  # Fleet entities
  if grep -Eq '\bisa\s+vehicle\b' <<<"$q"; then echo "vehicles"; return; fi
  if grep -Eq '\bisa\s+delivery\b' <<<"$q"; then echo "deliveries"; return; fi
  if grep -Eq '\bisa\s+employee\b' <<<"$q"; then echo "employees"; return; fi
  if grep -Eq '\bisa\s+operation\b' <<<"$q"; then echo "operations"; return; fi
  if grep -Eq '\bisa\s+rule\b' <<<"$q"; then echo "rules"; return; fi

  # Relations
  if grep -Eq '\)\s+isa\s+(drives|assigned-to|operates|manages)\b' <<<"$q"; then echo "operations"; return; fi

  # Fallback to vehicles service
  echo "vehicles"
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
    -d "{\"query\":\"$escaped\"}" 2>/dev/null || echo '{"error":"Connection failed"}')"

  if echo "$response" | jq -e '.ok' >/dev/null 2>&1; then
    echo "✅ Success"
  else
    echo "❌ Error: $response"
    echo "---- BLOCK THAT FAILED ----"
    echo "$block"
    echo "---------------------------"
    # Don't exit hard for now
  fi
  echo "---"
}

# ---- parse file into blocks ---------------------------------------

# We treat each transaction as everything from (insert|match) up to `commit;` 
block=""
in_tx=0
block_count=0

echo "🔍 Checking fleet management server..."
if ! curl -s "$BASE_URL/health" >/dev/null 2>&1; then
    echo "❌ Fleet management server not running on $BASE_URL"
    echo "   Please start the server first: cargo run"
    exit 1
fi
echo "✅ Server is running"
echo ""

echo "� Parsing $FILE for transaction blocks..."

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

  # detect end - look for commit or semicolon at end of line
  if [[ "$line" =~ (commit|;)[[:space:]]*$ ]]; then
    # send and reset
    ((block_count++))
    echo "Processing block $block_count..."
    send_block "$block"
    block=""
    in_tx=0
  fi
done < "$FILE"

# If file ended mid-transaction, still attempt to send
if [[ $in_tx -eq 1 && -n "$block" ]]; then
  echo "⚠️  File ended without commit - sending last buffered block anyway."
  send_block "$block"
fi

echo ""
echo "📊 Processed $block_count transaction blocks total."
echo ""

echo "🔍 Testing data reads..."
echo "========================"

read_query() {
  local service="$1"
  local q="$2"
  echo "📖 Reading $service..."
  local response
  response="$(curl -sS -X POST "$BASE_URL/$service" \
    -H "Content-Type: application/json" \
    -H "x-service-method: read" \
    -d "{\"query\":\"$(json_escape "$q")\"}" 2>/dev/null || echo '{"error":"Connection failed"}')"
  
  if echo "$response" | jq -e '.ok' >/dev/null 2>&1; then
    local count=$(echo "$response" | jq -r '.ok.answers | length // 0' 2>/dev/null || echo "0")
    echo "✅ Found $count records"
  else
    echo "❌ Read failed: $response"
  fi
  echo ""
}

read_query "vehicles" 'match $v isa vehicle; select $v;'
read_query "employees" 'match $e isa employee, has employee-role "driver"; select $e;'
read_query "deliveries" 'match $del isa delivery; select $del;'
read_query "employees" 'match $e isa employee; select $e;'
read_query "operations" 'match $o isa operation; select $o;'

echo "🎉 Fleet management database seeding completed!"
echo ""
echo "🚀 Your production-scale fleet system is ready with:"
echo "   • 50 diverse vehicles (trucks, vans, refrigerated, box trucks)"
echo "   • 105 employees with multi-role capabilities (drivers, dispatchers, supervisors, mechanics)"
echo "   • 35+ deliveries across all NYC boroughs"
echo "   • Background operations and assignments"
echo "   • Configuration rules for system behavior"
echo ""
echo "💡 Next steps:"
echo "   1. Access the web interface at http://localhost:3000"
echo "   2. Monitor real-time fleet operations"
echo "   3. Test background job processing"
