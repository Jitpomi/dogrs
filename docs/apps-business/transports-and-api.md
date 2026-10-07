# Transports & API Contracts

`apps/business` uses **`dog-transport`** to expose identical business service logic across multiple network protocols simultaneously.

---

## 1. HTTP / REST Endpoints

### Authentication (`/api/v1/auth`)
| Method | Endpoint | Description | Auth Required |
|---|---|---|---|
| `POST` | `/api/v1/auth/register` | Register new user account | No |
| `POST` | `/api/v1/auth/login` | Login with email and password | No |
| `POST` | `/api/v1/auth/refresh` | Exchange refresh token for new access token | No |
| `POST` | `/api/v1/auth/logout` | Revoke active access and refresh token | Yes (Bearer) |

### Organizations & Teams (`/api/v1/organizations`)
| Method | Endpoint | Description | Auth Required |
|---|---|---|---|
| `GET` | `/api/v1/organizations` | List organizations user belongs to | Yes |
| `POST` | `/api/v1/organizations` | Create a new organization | Yes |
| `GET` | `/api/v1/organizations/:org_id/members` | List members & inferred roles | Yes |
| `POST` | `/api/v1/organizations/:org_id/invites` | Invite new user with role | Yes (`admin`+) |

### Billing & Invoices (`/api/v1/billing`)
| Method | Endpoint | Description | Auth Required |
|---|---|---|---|
| `GET` | `/api/v1/billing/invoices` | List invoices for active tenant | Yes |
| `POST` | `/api/v1/billing/invoices` | Generate invoice for billing target | Yes (`billing:write`) |
| `POST` | `/api/v1/billing/webhooks` | Ingest external payment webhook | Signed Signature |

### Documents & Media (`/api/v1/documents`)
| Method | Endpoint | Description | Auth Required |
|---|---|---|---|
| `POST` | `/api/v1/documents/upload/init` | Initiate bounded multipart upload | Yes |
| `PUT` | `/api/v1/documents/upload/:upload_id` | Stream binary chunk to S3 | Yes |
| `POST` | `/api/v1/documents/upload/:upload_id/complete` | Finalize upload & commit TypeDB record | Yes |
| `GET` | `/api/v1/documents/:doc_id/download` | Stream document content from S3 | Yes |

---

## 2. Real-Time WebSocket Channels (`dog-transport`)

Real-time feeds are isolated strictly per tenant:

```text
ws://<host>/ws?token=<jwt>&tenant=<org_id>
```

### Channel Topics

| Topic | Event Payload | Description |
|---|---|---|
| `org:{org_id}:notifications` | `{"type": "toast", "message": "...", "severity": "info"}` | Direct in-app alerts and notifications. |
| `org:{org_id}:workflows` | `{"type": "task_updated", "task_id": "...", "status": "approved"}` | Real-time updates when an approval is processed. |
| `org:{org_id}:invoices` | `{"type": "invoice_paid", "invoice_id": "...", "amount": 1200.00}` | Real-time billing confirmation. |

---

## 3. Administrative CLI Commands

`apps/business` includes a CLI entrypoint for ops and migrations:

```bash
# Initialize and sync TypeDB schema & reasoning rules
cargo run -p business -- cli schema init

# Seed a new tenant organization with admin credentials
cargo run -p business -- cli tenant create --name "Acme Corp" --admin-email "admin@acme.com"

# Trigger batch billing generation across all active tenants
cargo run -p business -- cli billing cycle --run-date 2026-10-01
```
