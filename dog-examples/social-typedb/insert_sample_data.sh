#!/bin/bash

# Script to insert sample data using HTTP requests to our running application
# This tests our custom write methods with real data

echo "Inserting sample data via HTTP custom methods..."
echo "=============================================="

BASE_URL="http://127.0.0.1:3036"

# Function to insert data via write custom method
insert_data() {
    local service=$1
    local query=$2
    local description=$3
    
    echo "Inserting: $description"
    
    response=$(curl -s -X POST "$BASE_URL/$service" \
        -H "Content-Type: application/json" \
        -H "x-service-method: write" \
        -d "{\"query\": \"$query\"}")
    
    echo "Response: $response"
    echo "---"
}

# Insert Persons
echo "📝 Inserting Persons..."
insert_data "persons" "insert \$alice isa person, has username \"alice_smith\", has email \"alice@example.com\", has full-name \"Alice Smith\";" "Alice Smith"

insert_data "persons" "insert \$bob isa person, has username \"bob_jones\", has email \"bob@example.com\", has full-name \"Bob Jones\";" "Bob Jones"

insert_data "persons" "insert \$charlie isa person, has username \"charlie_brown\", has email \"charlie@example.com\", has full-name \"Charlie Brown\";" "Charlie Brown"

insert_data "persons" "insert \$diana isa person, has username \"diana_prince\", has email \"diana@example.com\", has full-name \"Diana Prince\";" "Diana Prince"

insert_data "persons" "insert \$eve isa person, has username \"eve_adams\", has email \"eve@example.com\", has full-name \"Eve Adams\";" "Eve Adams"

# Insert Organizations
echo "🏢 Inserting Organizations..."
insert_data "organizations" "insert \$techcorp isa organization, has name \"TechCorp Inc\", has organization-type \"company\";" "TechCorp Inc"

insert_data "organizations" "insert \$university isa organization, has name \"State University\", has organization-type \"educational-institute\";" "State University"

insert_data "organizations" "insert \$charity isa organization, has name \"Help Foundation\", has organization-type \"charity\";" "Help Foundation"

# Insert Groups
echo "👥 Inserting Groups..."
insert_data "groups" "insert \$developers isa group, has name \"Developers United\", has description \"A group for software developers\";" "Developers United"

insert_data "groups" "insert \$photographers isa group, has name \"Photo Enthusiasts\", has description \"Photography lovers community\";" "Photo Enthusiasts"

# Insert Posts
echo "📄 Inserting Posts..."
insert_data "posts" "insert \$post1 isa post, has title \"Welcome to TypeDB!\", has content \"Just started learning TypeDB and it's amazing!\", has created-at 2024-01-15T10:30:00;" "Welcome post"

insert_data "posts" "insert \$post2 isa post, has title \"Morning Run Complete\", has content \"Just finished a 5K run in the park. Beautiful weather today!\", has created-at 2024-01-15T08:15:00;" "Running post"

insert_data "posts" "insert \$post3 isa post, has title \"New Project Launch\", has content \"Excited to announce our new AI project at TechCorp!\", has created-at 2024-01-14T16:45:00;" "Project launch post"

# Insert Comments
echo "💬 Inserting Comments..."
insert_data "comments" "insert \$comment1 isa comment, has content \"Great post! TypeDB is indeed powerful.\", has created-at 2024-01-15T11:00:00;" "Comment 1"

insert_data "comments" "insert \$comment2 isa comment, has content \"I should start running too. Any tips for beginners?\", has created-at 2024-01-15T08:30:00;" "Comment 2"

echo ""
echo "🎉 Sample data insertion completed!"
echo ""
echo "Now let's test reading the data..."
echo "================================="

# Test reading data
echo "📖 Reading Persons..."
curl -s -X POST "$BASE_URL/persons" \
    -H "Content-Type: application/json" \
    -H "x-service-method: read" \
    -d '{"query": "match $person isa person; get $person;"}' | jq '.'

echo ""
echo "📖 Reading Posts..."
curl -s -X POST "$BASE_URL/posts" \
    -H "Content-Type: application/json" \
    -H "x-service-method: read" \
    -d '{"query": "match $post isa post; get $post;"}' | jq '.'

echo ""
echo "📖 Reading Organizations..."
curl -s -X POST "$BASE_URL/organizations" \
    -H "Content-Type: application/json" \
    -H "x-service-method: read" \
    -d '{"query": "match $org isa organization; get $org;"}' | jq '.'

echo ""
echo "✅ Sample data testing complete!"
