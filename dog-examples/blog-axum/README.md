# blog-axum (dog.rs example)

This is a small Axum HTTP server that showcases `dog-core` + `dog-axum`.

It demonstrates:

- **Service-first CRUD**: `POST /posts`, `GET /posts`, `GET /posts/:id`, `PATCH`, `DELETE`
- **Consistent JSON errors**: all errors return the `DogError` JSON shape
- **Request ID propagation**: every response includes `x-request-id`
- **Multi-tenancy**: data is isolated by `x-tenant-id`
- **Derived per-service params**: `includeDrafts=true` changes behavior for `GET /posts`

---

## Requirements

- Rust toolchain (stable)
- `curl`
- `jq` (optional, but makes output readable)

---

## 1) Run the server

From the workspace root:

```bash
cargo run -p blog-axum
```

It listens on:

- `http://127.0.0.1:3036`

---

## 2) Health check

```bash
curl -i http://127.0.0.1:3036/health
```

Expected:

- HTTP `200`
- response body: `ok`
- header: `x-request-id: <uuid>`

---

## 3) Create a post

### Create a published post (recommended for first test)

```bash
curl -i -X POST http://127.0.0.1:3036/posts \
  -H 'content-type: application/json' \
  -d '{"title":"Hello","body":"First post","published":true}'
```

Expected:

- HTTP `200`
- header: `x-request-id` exists
- JSON includes:
  - `id`
  - `title`, `body`
  - `published: true`
  - `createdAt`, `updatedAt`

### Create a draft post (defaults)

If you omit `published`, it defaults to `false`:

```bash
curl -i -X POST http://127.0.0.1:3036/posts \
  -H 'content-type: application/json' \
  -d '{"title":"Draft","body":"Not published yet"}'
```

---

## 4) View posts (published only by default)

### Default behavior (published only)

```bash
curl -s http://127.0.0.1:3036/posts | jq
```

Important:

- Drafts (`published=false`) are hidden by default.

### Include drafts too (derived PostParams)

```bash
curl -s 'http://127.0.0.1:3036/posts?includeDrafts=true' | jq
```

---

## 5) Multi-tenancy (x-tenant-id)

The same endpoints behave per-tenant. Data is isolated by the `x-tenant-id` request header.

### Create in tenant A

```bash
curl -i -X POST http://127.0.0.1:3036/posts \
  -H 'x-tenant-id: tenant-a' \
  -H 'content-type: application/json' \
  -d '{"title":"Tenant A post","body":"A","published":true}'
```

### Create in tenant B

```bash
curl -i -X POST http://127.0.0.1:3036/posts \
  -H 'x-tenant-id: tenant-b' \
  -H 'content-type: application/json' \
  -d '{"title":"Tenant B post","body":"B","published":true}'
```

### Verify isolation

Tenant A sees only tenant A posts:

```bash
curl -s http://127.0.0.1:3036/posts \
  -H 'x-tenant-id: tenant-a' | jq 'map(.title)'
```

Tenant B sees only tenant B posts:

```bash
curl -s http://127.0.0.1:3036/posts \
  -H 'x-tenant-id: tenant-b' | jq 'map(.title)'
```

If you omit `x-tenant-id`, the server uses the default tenant.

---

## 6) Validation errors (DogError JSON)

### Missing required field (422)

```bash
curl -i -X POST http://127.0.0.1:3036/posts \
  -H 'content-type: application/json' \
  -d '{"body":"missing title"}'
```

Expected:

- HTTP `422`
- JSON shape:
  - `name`, `className`, `code`, `message`, `errors`

### Malformed JSON (400)

```bash
curl -i -X POST http://127.0.0.1:3036/posts \
  -H 'content-type: application/json' \
  -d '{"title":}'
```

Expected:

- HTTP `400`
- `DogError` JSON shape

---

## 7) Run tests

From the workspace root:

```bash
cargo test -p blog-axum
```

Optional full workspace run:

```bash
cargo test --workspace
```
