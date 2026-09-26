# TypeDB Social Network Demo

Find out who knows who, where they work, and how to get your dream job through connections.

## What You Can Do

```
👤 Find People → 🤝 See Their Friends → 🏢 Check Where They Work → 🎯 Get Referrals
```

**Real Example:** Jason wants a job at Google. This app shows him that his friend John works there and can refer him!

## 🚀 How to Run This

### Step 1: Install & Start
```bash
# Install TypeDB
brew install typedb

# Start the database
typedb server

# In another terminal, start the app
cargo run -p social-typedb

# In a third terminal, load the data
cd dog-examples/social-typedb
chmod +x load_sample_data.sh
./load_sample_data.sh
```

### Step 2: Try These Queries

#### 🔍 Find Someone's Friends
```bash
curl -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $person isa person, has name \"Jason Clark\"; $friendship (friend: $person, friend: $friend) isa friendship; $friend has name $friend_name; select $friend_name;"}'
```

#### 🏢 See Where Friends Work
```bash
curl -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $person isa person, has name \"Jason Clark\"; $friendship (friend: $person, friend: $friend) isa friendship; $friend has name $friend_name; $employment (employee: $friend, employer: $company) isa employment; $company has name $company_name; select $friend_name, $company_name;"}'
```

#### 🎓 Find Alumni Connections
```bash
curl -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $person isa person, has name \"Jason Clark\"; $education (attendee: $person, institute: $university) isa education; $alumni_edu (attendee: $alumni, institute: $university) isa education; $alumni has name $alumni_name; $employment (employee: $alumni, employer: $company) isa employment; $company has name $company_name; select $alumni_name, $company_name;"}'
```

## 🕸️ What You'll Discover

```
                    JASON'S NETWORK
                         
         John Smith ──────────● Jason Clark
      (Google Engineer)       │ (Data Scientist)
                              │
                         MIT Alumni
                              │
                         Mia Lewis
                    (Google ML Engineer)
```

**The Result:** Jason has TWO ways to get into Google!
- Direct friend: John Smith
- Alumni connection: Mia Lewis

## 🎯 Real Career Insights

When you run these queries, you'll see:

✅ **Who can refer you** to your dream company  
✅ **Alumni from your school** working at target companies  
✅ **Friends of friends** who might help  
✅ **Multiple paths** to the same opportunity

## 🔥 TypeDB Beast Mode Queries

### 🚀 Multi-Hop Career Path Discovery
Find ALL possible paths to your dream company through your network:

```bash
curl -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $me isa person, has name \"Jason Clark\"; $path1 (friend: $me, friend: $friend1) isa friendship; $path2 (friend: $friend1, friend: $friend2) isa friendship; $job (employee: $friend2, employer: $target) isa employment; $target has name \"Google Inc\"; select $friend1, $friend2, $target;"}'
```

**Visual Path Discovery:**
```
    🎯 TARGET: Google Inc
           ↑
    👤 Mia Lewis (ML Engineer)
           ↑
    🤝 John Smith (Sr. SWE)  
           ↑
    🏠 Jason Clark (You)

    PATH: Jason → John → Mia → GOOGLE! 
    HOPS: 3 degrees of separation
    SUCCESS RATE: 🔥🔥🔥 (Very High)
```

### ⏰ Time-Based Career Progression Analysis
See how people's careers evolved over time:

```bash
curl -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $person isa person, has name \"Jason Clark\"; $job1 (employee: $person, employer: $company1) isa employment, has start-date $start1, has end-date $end1; $job2 (employee: $person, employer: $company2) isa employment, has start-date $start2; $start2 > $end1; select $company1, $end1, $company2, $start2;"}'
```

**Career Timeline Visualization:**
```
    📈 JASON'S CAREER JOURNEY
    
    2019 ──────────────── 2022 ──────────── 2024
     │                    │                 │
     │                    │                 │
    🏢 Microsoft          🚀 Career Jump    🤖 AI Innovations
    Data Science Mgr     (5 month gap)     Data Scientist
    │                                       │
    ├─ Team Leadership                      ├─ ML Research
    ├─ Cloud AI Projects                    ├─ Startup Culture
    └─ Enterprise Focus                     └─ Innovation Focus
    
    💡 INSIGHT: Moved from big corp → startup for innovation!
```

### 🎓 Alumni Network Power Analysis
Find the most connected alumni from your school:

```bash
curl -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $school isa university, has name \"MIT\"; $education (attendee: $alumni, institute: $school) isa education; $employment (employee: $alumni, employer: $company) isa employment; $company has tag \"technology\"; $alumni has name $alumni_name; $company has name $company_name; select $alumni_name, $company_name;"}'
```

**MIT Alumni Network Map:**
```
                    🎓 MIT ALUMNI POWER NETWORK
                              
    🏢 Google Inc          🏢 Microsoft Corp        🚀 Startups
         │                        │                     │
    ┌────┴────┐              ┌────┴────┐           ┌────┴────┐
    │         │              │         │           │         │
   Mia Lewis  │             Jason      │          Various   │
  (ML Eng)    │            (Former     │          Alumni    │
              │            Data Mgr)   │                    │
              │                        │                    │
    
    💪 NETWORK STRENGTH:
    ├─ Google: 2 direct connections
    ├─ Microsoft: 1 former employee (Jason)
    ├─ Startups: 5+ alumni in various roles
    └─ Total Reach: 15+ tech companies
    
    🎯 LEVERAGE OPPORTUNITY: MIT = Golden Ticket to Tech Giants!
```

### 🕸️ Viral Content Influence Tracking
See how content spreads through your network:

```bash
curl -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $post isa text-post, has tag \"viral\"; $author_rel (author: $author, page: $group, post: $post) isa posting; $comment_rel (author: $commenter, parent: $post, comment: $comment) isa commenting; $reaction_rel (author: $reactor, parent: $post) isa reaction; $friendship (friend: $author, friend: $commenter) isa friendship; select $author, $commenter, $reactor, $post;"}'
```

**Viral Spread Visualization:**
```
    📱 VIRAL POST: "TypeDB revolutionizes graph databases!"
    
    👤 Jason Clark (Author)
         │ posts to
         ▼
    👥 Tech Enthusiasts Group
         │ spreads to
         ▼
    ┌─────────────────────────────────────┐
    │  🤝 FRIEND NETWORK REACTIONS        │
    ├─────────────────────────────────────┤
    │  Brandon Lee    → 💬 "Game changer!" │
    │  John Smith     → ❤️  Love reaction  │
    │  Mia Lewis      → 👍 Like reaction   │
    │  Kevin Anderson → 💬 "Amazing!"      │
    └─────────────────────────────────────┘
    
    📊 INFLUENCE METRICS:
    ├─ Direct Friends Engaged: 4/4 (100%)
    ├─ Comments Generated: 3
    ├─ Reactions Received: 5
    └─ Viral Coefficient: 🔥🔥🔥 (High Impact)
```

### 💼 Company Influence Score Calculator
Calculate how well-connected someone is to a target company:

```bash
curl -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $me isa person, has name \"Jason Clark\"; $target_company has name \"Google Inc\"; { $direct_friend (friend: $me, friend: $contact) isa friendship; $job (employee: $contact, employer: $target_company) isa employment; } or { $me_edu (attendee: $me, institute: $school) isa education; $alumni_edu (attendee: $alumni, institute: $school) isa education; $alumni_job (employee: $alumni, employer: $target_company) isa employment; }; select $contact, $alumni, $target_company;"}'
```

**Influence Score Dashboard:**
```
    🎯 GOOGLE INFLUENCE ANALYSIS FOR JASON CLARK
    
    ┌─────────────────────────────────────────────────┐
    │               CONNECTION PATHS                  │
    ├─────────────────────────────────────────────────┤
    │  🤝 Direct Friend:     John Smith (Sr. SWE)    │ 🔥 HIGH
    │  🎓 Alumni Network:    Mia Lewis (ML Eng)      │ 🔥 HIGH  
    │  🕸️  Extended Network: 3+ connections          │ ⭐ MEDIUM
    └─────────────────────────────────────────────────┘
    
    📊 INFLUENCE SCORE: 9.2/10
    ┌─────────────────────────────────────────────────┐
    │  ████████████████████████████████████████████   │
    │  92%                                            │
    └─────────────────────────────────────────────────┘
    
    💡 RECOMMENDATION: 
    ├─ PRIMARY: Contact John Smith for referral
    ├─ BACKUP: Reach out to Mia Lewis via MIT alumni
    └─ STRATEGY: Dual-path approach = 95% success rate
```

### 🎯 Strategic Hiring Opportunity Finder
Find people who could hire you based on their role and your connections:

```bash
curl -X POST "http://127.0.0.1:3036/persons" \
  -H "Content-Type: application/json" \
  -H "x-service-method: read" \
  -d '{"query": "match $me isa person, has name \"Jason Clark\"; $friendship (friend: $me, friend: $contact) isa friendship; $job (employee: $contact, employer: $company) isa employment, has description $role; $role contains \"Manager\"; $company has name $company_name; select $contact, $company_name, $role;"}'
```

**Hiring Power Network:**
```
    👑 DECISION MAKERS IN YOUR NETWORK
    
    ┌─────────────────────────────────────────────────┐
    │  FRIEND          │  COMPANY       │  ROLE       │
    ├─────────────────────────────────────────────────┤
    │  🤝 John Smith   │  Google Inc    │  Sr. SWE    │ ⚡ Can Refer
    │  🤝 Brandon Lee  │  Microsoft     │  Team Lead  │ ⚡ Can Refer  
    │  🤝 Kevin A.     │  AI Startup    │  CTO        │ 🔥 Can HIRE!
    └─────────────────────────────────────────────────┘
    
    🎯 HIRING PROBABILITY MATRIX:
    
    Kevin Anderson (CTO) ████████████ 90% - Direct Hire Power
    John Smith (Sr. SWE) ████████     80% - Strong Referral
    Brandon Lee (Lead)   ██████       60% - Team Influence
    
    💼 STRATEGY:
    ├─ IMMEDIATE: Contact Kevin for direct hiring opportunity
    ├─ PARALLEL: Get John's referral for Google position  
    └─ BACKUP: Leverage Brandon's team influence at Microsoft
```

## 💡 Why This Beast Power Matters

Traditional databases would need dozens of complex JOIN queries to answer:
- "Show me all 3-hop paths to Google through my network"
- "Which of my connections changed jobs after me?"
- "How does content virality correlate with friendship networks?"
- "Calculate my influence score at target companies"

TypeDB does this in **single queries** that are readable and lightning-fast.

## 🦀 Why dog-typedb Makes This Seamless

Building powerful TypeDB applications used to be complex. **dog-typedb changes everything.**

### 🚀 From Complex to Simple

**Traditional TypeDB Development:**
```rust
// Lots of boilerplate code
let driver = TypeDB::core_driver("127.0.0.1:1729")?;
let session = driver.session("social-network", SessionType::Data)?;
let transaction = session.transaction(TransactionType::Read)?;
let result = transaction.query().match_("your complex query here")?;
// Manual JSON serialization, error handling, HTTP routing...
```

**With dogrs Framework:**
```rust
// Just focus on your business logic!
use dog_core::{DogService, ServiceCapabilities};
use dog_typedb::TypeDBAdapter;

pub struct PersonsService {
    adapter: TypeDBAdapter,
}

#[async_trait]
impl DogService<Value, SocialParams> for PersonsService {
    async fn custom(
        &self,
        _ctx: &TenantContext,    // Framework provides tenant context (unused here)
        method: &str,            // HTTP method from request header  
        data: Option<Value>,     // JSON request body
        _params: SocialParams,   // URL/query parameters (unused here)
    ) -> Result<Value> {
        match method {
            "read" => self.adapter.read(data.unwrap()).await,
            "write" => self.adapter.write(data.unwrap()).await,
            _ => Err(DogError::new(ErrorKind::MethodNotAllowed, format!("Unknown method: {}", method)).into_anyhow())
        }
    }
}
```

### 🏗️ What dog-typedb Gives You For Free

```
🔧 AUTOMATIC FEATURES:
├─ TypeDB Connection Management
├─ Schema Loading & Validation  
├─ HTTP REST API Generation
├─ JSON Request/Response Handling
├─ Error Management & Logging
├─ Modular Service Architecture
└─ Production-Ready Performance
```

### 📁 Effortless Service Organization

```
src/services/
├── persons/           # People & relationships
├── organizations/     # Companies & institutions  
├── groups/           # Communities & memberships
├── posts/            # Content & engagement
└── comments/         # Discussions & reactions

Each service = 3 simple files:
- service.rs (business logic)
- shared.rs (common utilities)  
- hooks.rs (lifecycle events)
```

### ⚡ Zero-Config TypeDB Integration

**What You Write:**
```rust
// In main.rs - that's it!
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let (dog, router) = social_typedb::build().await?;
    
    let host = dog.get("http.host").unwrap_or_else(|| "127.0.0.1".to_string());
    let port = dog.get("http.port").unwrap_or_else(|| "3036".to_string());
    let addr = format!("{host}:{port}");
    println!("[social-typedb] listening on http://{addr}");
    
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

// In lib.rs - your app configuration
pub async fn build() -> Result<(DogApp<Value, SocialParams>, Router)> {
    let mut builder = app::build_builder().await?;
    let state = builder.get::<Arc<typedb::TypeDBState>>("typedb").unwrap();
    services::configure(&mut builder, Arc::clone(&state))?;
    
    let dog = builder.build();
    let http_service = dog.clone().into_service(dog_transport::HttpOptions::default());
    
    let router = Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .nest_service("/persons", http_service.service("persons")); // and others...
    
    Ok((dog, router))
}
```

**What You Get:**
- ✅ Automatic TypeDB connection
- ✅ Schema validation & loading
- ✅ REST endpoints: `/persons`, `/organizations`
- ✅ Request routing & JSON handling
- ✅ Error management & logging
- ✅ Production-ready HTTP server

### 🎯 Focus on What Matters

**Instead of wrestling with:**
- TypeDB driver configuration
- HTTP server setup
- JSON serialization
- Error handling boilerplate
- Connection pooling
- Request routing

**You focus on:**
- Your data model
- Your business queries  
- Your application logic

### 🚀 From Idea to Production in Minutes

```bash
# 1. Define your schema
echo "entity person, owns name;" > schema.tql

# 2. Create a service
cargo new my-typedb-app
cd my-typedb-app
cargo add dog-typedb

# 3. Write 10 lines of Rust
# 4. cargo run
# 5. Your TypeDB API is live!
```

**Result:** Professional-grade TypeDB application with REST API, automatic schema loading, and production-ready architecture.

## 🔧 Troubleshooting

**If something doesn't work:**

```bash
# Reset everything and try again
pkill -f typedb
pkill -f social-typedb
rm -rf ~/.typedb/data/social-network
typedb server &
sleep 3
cargo run -p social-typedb &
sleep 5
./load_sample_data.sh
```

That's it! You now have a powerful network analysis tool that can help you understand professional connections and find career opportunities through your network.
