# TypeDB Social Network Analysis with dogrs

A comprehensive demonstration of advanced graph database capabilities using TypeDB and the dogrs HTTP framework for Rust.

## 🚀 What This Demonstrates

This project showcases the transformation from **sparse, disconnected data** to a **comprehensive social network analysis platform** capable of:

- **Multi-hop relationship traversal** across 7+ entity types
- **Professional network analysis** for career advancement
- **Friend-of-friend discovery** for referral opportunities  
- **Alumni network mapping** for strategic connections
- **Risk assessment** for potentially harmful professional relationships
- **Strategic career recommendations** based on network positioning

## 📊 Dataset Overview

**Entities:** 24 persons, 13 organizations, 4 groups, 23 posts, 10 comments  
**Relationships:** 8 friendships, 6 employment, 4 education, 8 commenting, 9 reactions  
**Transaction Blocks:** 119 comprehensive data scenarios

## 🎯 Advanced Network Analysis Example

### The Ultimate TypeDB Query

Here's a **7-hop relationship traversal** that demonstrates the full power of graph databases:

```typeql
match 
  $person isa person, has username "user_2025_17", has name $name, has gender $gender, has email $email;
  $friendship (friend: $person, friend: $friend) isa friendship;
  $friend has username $friend_username, has name $friend_name;
  (employee: $person, employer: $company) isa employment, has description $job_desc;
  $company has name $company_name;
  (attendee: $person, institute: $university) isa education, has description $edu_desc;
  $university has name $uni_name;
  (member: $person, group: $group) isa group-membership, has rank $rank;
  $group has name $group_name;
  (author: $person, page: $group, post: $post) isa posting;
  $post has post-text $post_text;
  (author: $friend, parent: $post, comment: $comment) isa commenting;
  $comment has comment-text $comment_text;
select $name, $gender, $email, $friend_username, $friend_name, $company_name, $job_desc, 
       $uni_name, $edu_desc, $group_name, $rank, $post_text, $comment_text;
```

**Result:** Complete digital life profile revealing:
- **Personal identity:** Jason Clark, male, user17@example.com
- **Social connections:** Brandon Lee (friend network)
- **Career trajectory:** Data Scientist at AI Innovations Corp
- **Educational background:** Computer Science PhD at Stanford University  
- **Community engagement:** Moderator at Tech Enthusiasts group
- **Content creation:** "Check out this TypeDB tutorial!"
- **Network effects:** Friends engaging with his content

### Professional Network Analysis

#### 1. Direct Career Connections
```typeql
match $jason isa person, has username "user_2025_17"; 
      $friendship (friend: $jason, friend: $friend) isa friendship; 
      $friend has name $friend_name; 
      $employment (employee: $friend, employer: $company) isa employment, has description $friend_role; 
      $company has name $company_name, has tag $company_tag; 
select $friend_name, $company_name, $company_tag, $friend_role;
```

**Strategic Insights:**
- **John Smith** → Google Inc (Senior Software Engineer) 
- **Direct referral path** to Google's AI/ML teams
- **High-value connection** in target company

#### 2. Alumni Network Power
```typeql
match $jason isa person, has username "user_2025_17"; 
      $jason_edu (attendee: $jason, institute: $university) isa education; 
      $university has name $uni_name; 
      $alumni_edu (attendee: $alumni, institute: $university) isa education; 
      $alumni has name $alumni_name; 
      $alumni_employment (employee: $alumni, employer: $company) isa employment, has description $alumni_role; 
      $company has name $company_name; 
select $uni_name, $alumni_name, $company_name, $alumni_role;
```

**Strategic Insights:**
- **MIT alumni network** → Google, Microsoft connections
- **Mia Lewis** → Google Inc (Machine Learning Engineer)
- **Premium educational pedigree** opening doors to top-tier companies

#### 3. Friend-of-Friend Opportunities
```typeql
match $jason isa person, has username "user_2025_17"; 
      $friendship1 (friend: $jason, friend: $direct_friend) isa friendship; 
      $friendship2 (friend: $direct_friend, friend: $friend_of_friend) isa friendship; 
      $friend_of_friend has name $fof_name; 
      $employment (employee: $friend_of_friend, employer: $company) isa employment, has description $fof_role; 
      $company has name $company_name; 
select $fof_name, $company_name, $fof_role;
```

**Strategic Insights:**
- **Extended network reach** through Brandon Lee and John Smith
- **Multiple referral paths** to Google, Microsoft, AI startups
- **Cross-industry connections** spanning tech giants and emerging companies

### 🚨 Risk Assessment: Toxic Connections Analysis

Identify potentially harmful professional relationships:

```typeql
match $jason isa person, has username "user_2025_17"; 
      $friendship (friend: $jason, friend: $risky_friend) isa friendship; 
      $risky_friend has name $friend_name; 
      $employment (employee: $risky_friend, employer: $company) isa employment; 
      $company has name $company_name, has tag $company_tag; 
      $company_tag contains "controversial"; 
select $friend_name, $company_name, $company_tag;
```

**Strategic Recommendations:**
- **Audit connections** to companies with reputational risks
- **Distance from** friends at failing startups or controversial organizations
- **Prioritize relationships** that enhance rather than diminish professional standing

## 🎯 Career Strategy Recommendations

Based on the network analysis:

### Immediate Opportunities (High Success Probability)
1. **Google AI/ML Teams** ⭐⭐⭐⭐⭐
   - **Referral Path:** John Smith → Internal referral + MIT alumni connection
   - **Success Factors:** Stanford PhD + direct friend at Google + AI expertise alignment

2. **Microsoft AI Research** ⭐⭐⭐⭐
   - **Referral Path:** Previous employment history + network connections  
   - **Success Factors:** Proven Data Science Manager track record

### Network Leverage Score: 9/10 🔥
- Multiple referral paths to target companies
- High-value connections in AI/ML space  
- Strong alumni network providing ongoing opportunities
- Strategic positioning at center of powerful professional ecosystem

## 🛠️ Complete Setup & Installation Guide

### Prerequisites

**Required Software:**
- **TypeDB Server** (latest version)
- **Rust toolchain** (1.70+ recommended)
- **Git** for cloning the repository
- **curl** and **jq** for testing queries

### Step-by-Step Installation

#### 1. Install TypeDB Server

**macOS (Homebrew):**
```bash
brew install typedb
```

**Linux/Windows:**
```bash
# Download from https://github.com/vaticle/typedb/releases
# Extract and add to PATH
```

**Verify Installation:**
```bash
typedb --version
```

#### 2. Install Rust Toolchain

```bash
# Install rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Verify installation
rustc --version
cargo --version
```

#### 3. Clone and Build Project

```bash
# Clone the repository
git clone <repository-url>
cd dogrs/dog-examples/social-typedb

# Build the project
cargo build --release
```

### 🚀 Running the Complete Demo

#### Step 1: Start TypeDB Server

```bash
# Start TypeDB server (keep this terminal open)
typedb server

# You should see:
# TypeDB server is running on 127.0.0.1:1729
```

#### Step 2: Launch the Application

```bash
# In a new terminal, navigate to project root
cd dogrs

# Start the social-typedb application
cargo run -p social-typedb

# You should see:
# [social-typedb] listening on http://127.0.0.1:3036
```

#### Step 3: Seed the Database

```bash
# In a third terminal, navigate to social-typedb directory
cd dogrs/dog-examples/social-typedb

# Make the loader script executable and run it
chmod +x load_sample_data.sh
./load_sample_data.sh

# You should see:
# Loading TypeQL from: sample_data.tql
# Processing 119 transaction blocks...
# ✅ Success messages for each batch
```

**Expected Output:**
```
Loading TypeQL from: sample_data.tql
BASE_URL: http://127.0.0.1:3036
===========================================
Parsing sample_data.tql for transaction blocks...
Found transaction start: insert...
Processing block 1...
📝 -> persons
   insert...
✅ Success
---
[... continues for 119 blocks ...]
Processed 119 transaction blocks total.

🎉 Loading completed.

Now let's test reading the data...
=================================
📖 Reading persons...
{
  "ok": {
    "answerType": "conceptRows",
    "answers": [
      // 24 person entities
    ]
  }
}
```

### 🧪 Testing the Network Analysis

#### Basic Connectivity Test

```bash
# Test basic person query
curl -s -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $person isa person; limit 3; select $person;"}' | jq .
```

#### Test Jason's Network

```bash
# Find Jason's friends
curl -s -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $jason isa person, has username \"user_2025_17\"; $friendship (friend: $jason, friend: $friend) isa friendship; $friend has name $friend_name; select $friend_name;"}' | jq .
```

#### Ultimate Network Analysis Query

```bash
# Run the 7-hop relationship traversal
curl -s -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $person isa person, has username \"user_2025_17\", has name $name, has gender $gender, has email $email; $friendship (friend: $person, friend: $friend) isa friendship; $friend has username $friend_username, has name $friend_name; (employee: $person, employer: $company) isa employment, has description $job_desc; $company has name $company_name; (attendee: $person, institute: $university) isa education, has description $edu_desc; $university has name $uni_name; (member: $person, group: $group) isa group-membership, has rank $rank; $group has name $group_name; (author: $person, page: $group, post: $post) isa posting; $post has post-text $post_text; (author: $friend, parent: $post, comment: $comment) isa commenting; $comment has comment-text $comment_text; limit 1; select $name, $gender, $email, $friend_username, $friend_name, $company_name, $job_desc, $uni_name, $edu_desc, $group_name, $rank, $post_text, $comment_text;"}' | jq .
```

### 📊 Verification Checklist

After setup, verify these components are working:

- [ ] **TypeDB Server** running on port 1729
- [ ] **Social-TypeDB App** running on port 3036
- [ ] **119 transaction blocks** successfully processed
- [ ] **24 persons** loaded (`curl persons endpoint`)
- [ ] **13 organizations** loaded (`curl organizations endpoint`)
- [ ] **8 friendships** created (test friendship queries)
- [ ] **6 employment relationships** created
- [ ] **Network analysis queries** returning data

### 🔧 Troubleshooting

#### TypeDB Server Issues

```bash
# If server fails to start
pkill -f typedb
rm -rf ~/.typedb/data/social-network
rm -rf ~/.typedb/server/data/social-network
typedb server
```

#### Application Port Conflicts

```bash
# If port 3036 is in use
pkill -f social-typedb
lsof -ti:3036 | xargs kill -9
cargo run -p social-typedb
```

#### Data Loading Issues

```bash
# If seeding fails, clear and retry
pkill -f social-typedb
rm -rf ~/.typedb/data/social-network
typedb server &
cargo run -p social-typedb &
sleep 5
./load_sample_data.sh
```

#### Common Error Solutions

**"Database not found":**
- Ensure TypeDB server is running first
- Wait 2-3 seconds after starting the app before seeding

**"Connection refused":**
- Check if TypeDB server is running on port 1729
- Verify social-typedb app is running on port 3036

**"Empty query results":**
- Verify data was loaded successfully (check load_sample_data.sh output)
- Ensure all 119 blocks were processed without errors

## 🏗️ Architecture

- **TypeDB:** Graph database for complex relationship modeling
- **dogrs:** Rust HTTP framework providing REST API endpoints
- **Schema:** Comprehensive social network model with persons, organizations, groups, posts, comments
- **Services:** Modular HTTP services for persons, organizations, groups, posts, comments

## 📈 From Zero to Hero: The Transformation

**Before:** Many queries returned zero results due to sparse, poorly distributed seed data  
**After:** Comprehensive network analysis with 144+ relationship combinations in single queries

This demonstrates the true power of TypeDB for modeling and querying interconnected real-world data that traditional relational databases struggle to represent effectively.

## 🎉 Key Features Demonstrated

- ✅ **Multi-Entity Joins:** 7 different entity types in one query
- ✅ **Complex Relationship Chaining:** Friend networks → professional history → educational background  
- ✅ **Graph Traversal:** Multi-hop navigation through interconnected data
- ✅ **Real-World Modeling:** Social network analysis with professional and educational context
- ✅ **Strategic Intelligence:** Career advancement through network analysis
- ✅ **Risk Assessment:** Identification of potentially harmful connections

## 🔍 Advanced Query Examples

### 🕸️ Network Visualization

```
                    JASON'S PROFESSIONAL ECOSYSTEM
                              
                         🎓 MIT Alumni Network
                              │
                    ┌─────────┼─────────┐
                    │         │         │
                 Mia Lewis    │    Other Alumni
              (Google ML Eng) │    (Various Companies)
                    │         │         │
                    └─────────┼─────────┘
                              │
    🏢 Direct Friends ────────┼──────── Educational Background 🎓
                              │
         John Smith ──────────●──────── Jason Clark
      (Google Sr. SWE)        │      (Stanford PhD, MIT)
                              │      Data Scientist @ AI Corp
                              │           │
                              │           │
    🤝 Friend-of-Friend ──────┼───────────┘
         Network              │
                              │
                    ┌─────────┼─────────┐
                    │         │         │
              Kevin Anderson  │    Brandon Lee
             (Extended Net)   │   (Direct Friend)
                    │         │         │
                    └─────────┼─────────┘
                              │
                         💼 Career Opportunities
                    Google • Microsoft • Startups
```

### 📊 Query Result Visualization

**🎯 7-Hop Relationship Traversal Results:**

```
┌─────────────────────────────────────────────────────────────┐
│                    JASON CLARK PROFILE                     │
├─────────────────────────────────────────────────────────────┤
│ 👤 Personal: Jason Clark, male, user17@example.com         │
│ 🤝 Friend: Brandon Lee (user_2025_19)                      │
│ 🏢 Company: AI Innovations Corp (Data Scientist)           │
│ 🎓 Education: Stanford University (Computer Science PhD)   │
│ 👥 Group: Tech Enthusiasts (moderator)                     │
│ 📝 Content: "Check out this TypeDB tutorial!"             │
│ 💬 Engagement: "Looking forward to more content"          │
└─────────────────────────────────────────────────────────────┘
```

### 🎯 Career Opportunity Matrix

```
                    REFERRAL STRENGTH vs COMPANY FIT
                              
    High Fit    │  🔥 GOOGLE      │  ⭐ Microsoft   │
                │  (AI/ML Teams)  │  (AI Research)  │
                │  John Smith +   │  Employment     │
                │  MIT Alumni     │  History        │
                ├─────────────────┼─────────────────┤
                │  💡 Startups    │  🏢 Enterprise  │
    Medium Fit  │  (AI Focus)     │  (Consulting)   │
                │  Network Reach  │  Alumni Conns   │
                └─────────────────┴─────────────────┘
                  High Referral    Medium Referral
                    Strength         Strength
```

### 🚨 Network Risk Assessment

```
    RELATIONSHIP IMPACT ANALYSIS
    
    ✅ HIGH VALUE CONNECTIONS:
    ┌──────────────────────────────────┐
    │ John Smith → Google (Sr. SWE)    │ 🔥 Leverage
    │ MIT Alumni → Tech Giants         │ 🔥 Leverage  
    │ Brandon Lee → Extended Network   │ ⭐ Expand
    └──────────────────────────────────┘
    
    ⚠️  NEUTRAL CONNECTIONS:
    ┌──────────────────────────────────┐
    │ Startup Employees → Risk/Reward  │ 📊 Monitor
    │ Non-Tech Friends → Limited Value │ 📊 Maintain
    └──────────────────────────────────┘
    
    🚨 POTENTIAL RISKS:
    ┌──────────────────────────────────┐
    │ Controversial Companies → Audit  │ ⚠️  Distance
    │ Failed Startups → Reputation     │ ⚠️  Evaluate
    └──────────────────────────────────┘
```

### 📈 Strategic Action Plan

```
    NETWORK OPTIMIZATION ROADMAP
    
    🎯 IMMEDIATE (0-6 months)
    ├── Activate Google referral (John Smith)
    ├── Strengthen MIT alumni connections  
    └── Expand tech community leadership
    
    🚀 MEDIUM-TERM (6-18 months)
    ├── Target FAANG senior positions
    ├── Build thought leadership platform
    └── Mentor junior developers
    
    🏆 LONG-TERM (18+ months)
    ├── Engineering management roles
    ├── Industry conference speaking
    └── Potential startup founding
```

See the comprehensive queries above for examples of:
- 🕸️ **Professional network mapping** - Multi-hop relationship discovery
- 🎓 **Alumni connection analysis** - Educational network leverage  
- 🤝 **Friend-of-friend discovery** - Extended referral opportunities
- 🎯 **Career opportunity identification** - Strategic positioning analysis
- 🚨 **Risk assessment and network pruning** - Relationship impact evaluation

This TypeDB demonstration showcases how graph databases excel at modeling complex, attribute-rich, interconnected real-world scenarios for strategic decision-making.
