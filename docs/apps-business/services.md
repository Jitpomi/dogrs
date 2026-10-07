# Domain Services Specification

All services in `apps/business` adhere strictly to the [DogRS Application Structure](../../docs/application-structure.md).

```text
src/services/
├── mod.rs
├── types.rs
├── iam/
│   ├── mod.rs
│   ├── iam_service.rs
│   ├── iam_hooks.rs
│   ├── iam_shared.rs
│   └── iam_schema.rs
├── organizations/
│   ├── mod.rs
│   ├── organizations_service.rs
│   ├── organizations_hooks.rs
│   ├── organizations_shared.rs
│   └── organizations_schema.rs
├── billing/
│   ├── mod.rs
│   ├── billing_service.rs
│   ├── billing_hooks.rs
│   ├── billing_shared.rs
│   └── billing_schema.rs
├── documents/
│   ├── mod.rs
│   ├── documents_service.rs
│   ├── documents_hooks.rs
│   ├── documents_shared.rs
│   └── documents_schema.rs
└── workflows/
    ├── mod.rs
    ├── workflows_service.rs
    ├── workflows_hooks.rs
    ├── workflows_shared.rs
    └── workflows_schema.rs
```

---

## 1. `iam` (Identity & Access Management)

### Responsibilities
- User registration, local credential validation, and OAuth2 social login.
- JWT access and refresh token generation (powered by `dog-auth` and `aws-lc-rs`).
- Dynamic token revocation and session blacklisting.

### Module Breakdown
- **`iam_service.rs`**: Implements `DogService` for operations:
  - `register_user(email, password, name)`
  - `authenticate_password(email, password)`
  - `refresh_token(refresh_token)`
  - `revoke_session(jti)`
- **`iam_hooks.rs`**: Pre-operation hooks enforcing password strength and verifying active account status.
- **`iam_schema.rs`**: Schemas validating email format, password complexity, and token request structures.

---

## 2. `organizations` (Multi-Tenancy & Teams)

### Responsibilities
- Organization lifecycle (create, update, suspend).
- Subsidiary hierarchy linking (`parent_org` -> `child_org`).
- Member invitations, role assignments (`owner`, `admin`, `manager`, `member`), and permission resolution via TypeDB reasoning.

### Module Breakdown
- **`organizations_service.rs`**:
  - `create_organization(name, owner_user_id)`
  - `link_subsidiary(parent_org_id, child_org_id)`
  - `invite_member(org_id, email, role)`
  - `get_user_permissions(org_id, user_id)`
- **`organizations_hooks.rs`**: Enforces that operations match the caller's active `TenantContext`.

---

## 3. `billing` (Invoicing & Subscriptions)

### Responsibilities
- Subscription tier management (Free, Pro, Enterprise).
- Invoice generation, tax calculation, and payment reconciliation.
- Asynchronous billing webhook processing via `dog-queue`.

### Module Breakdown
- **`billing_service.rs`**:
  - `create_invoice(org_id, amount, line_items)`
  - `process_payment_webhook(provider_payload)`
  - `get_tenant_billing_status(org_id)`
- **`billing_hooks.rs`**: Enqueues invoice PDF generation and receipt delivery jobs onto `dog-queue`.
- **`billing_schema.rs`**: Validates monetary amounts, currency codes, and webhook signatures.

---

## 4. `documents` (Contracts & Files)

### Responsibilities
- Tenant-scoped file metadata records in TypeDB.
- Streaming uploads and downloads via `dog-blob` (backed by S3).
- Resumable multipart upload session management.

### Module Breakdown
- **`documents_service.rs`**:
  - `initiate_upload(org_id, file_name, mime_type, file_size)`
  - `commit_upload(org_id, doc_id, blob_key)`
  - `generate_download_stream(org_id, doc_id)`
- **`documents_hooks.rs`**: Enforces maximum upload byte boundaries and checks virus/MIME validation.

---

## 5. `workflows` (Approvals & Task State Machines)

### Responsibilities
- Multi-step approval workflows for high-value invoices and contracts.
- Task assignments, deadlines, and state transitions (`pending` -> `approved` | `rejected`).
- Real-time event broadcasting over WebSockets via `dog-transport`.

### Module Breakdown
- **`workflows_service.rs`**:
  - `create_task(org_id, title, assignee_id, target_doc_id)`
  - `submit_approval(org_id, task_id, approver_id, decision)`
- **`workflows_hooks.rs`**: Publishes state transition events to `org:{org_id}:workflows` WebSocket channel.
