# Todo List API: Rust + PostgreSQL (Docker Compose)

A small, unauthenticated Todo API written in Rust with Axum and SQLx, backed by PostgreSQL.
Everything runs with Docker Compose. Todos are stored in a named volume, so they survive restarts.

## Stack

| Part | Choice |
|---|---|
| Language and framework | Rust, Axum 0.7 |
| Database access | SQLx 0.8 with embedded migrations |
| Database | PostgreSQL 16 (official `postgres:16-alpine` image) |
| Orchestration | Docker Compose |

## Run it

```bash
docker compose up --build
```

The API and a simple web page are then available at **http://localhost:3000**.
Open that address in a browser to add, tick, edit and delete todos.

Optional: change the database credentials first.

```bash
cp .env.example .env     # then edit the password
docker compose up --build
```

If there is no `.env` file, safe development defaults are used.

## Environment variables

| Variable | Used by | Default | Purpose |
|---|---|---|---|
| `POSTGRES_USER` | db, api | `todo` | Database user |
| `POSTGRES_PASSWORD` | db, api | `todopass` | Database password (change it in `.env`) |
| `POSTGRES_DB` | db, api | `tododb` | Database name |
| `RUST_LOG` | api | `info` | Log level |
| `DATABASE_URL` | api | built by Compose | Connection string, never hardcoded in source |
| `PORT` | api | `3000` | Port the API listens on inside the container |

## Endpoints

| Method | Path | Description | Success |
|---|---|---|---|
| GET | `/` | The web frontend (a single HTML page) | 200 |
| GET | `/health` | Checks the API and the database | 200 |
| GET | `/todos` | List all todos | 200 |
| POST | `/todos` | Create a todo | 201 |
| GET | `/todos/:id` | Get one todo | 200 |
| PUT | `/todos/:id` | Update one todo (send `title`, `completed`, or both) | 200 |
| DELETE | `/todos/:id` | Delete one todo | 204 |

A todo looks like `{"id": 1, "title": "Learn Rust", "completed": false}`.

Errors are JSON, for example `{"error": "todo not found"}`:
- `400` for a missing or empty title, invalid JSON, or an id that is not a positive integer
- `404` for an id that does not exist or an unknown path
- `500` only for real database failures

## Try it

```bash
curl http://localhost:3000/health

curl -X POST http://localhost:3000/todos \
  -H "Content-Type: application/json" \
  -d '{"title":"Learn Rust"}'

curl http://localhost:3000/todos
curl http://localhost:3000/todos/1

curl -X PUT http://localhost:3000/todos/1 \
  -H "Content-Type: application/json" \
  -d '{"completed":true}'

curl -X DELETE http://localhost:3000/todos/1
```

## How it is built

**Frontend.** `static/index.html` is plain HTML, CSS and JavaScript, compiled into the binary with `include_str!`.
It needs no extra container. It calls the API with relative paths (`todos`, not `/todos`), so it also works behind
Nginx at `/rust-todos/`.

**Multi-stage Dockerfile.** Stage 1 (`rust:1-bookworm`) compiles the app. Stage 2 (`debian:bookworm-slim`)
receives only the compiled binary, so the toolchain is not shipped. Dependencies are compiled in their own
cached layer, so changing source code does not recompile them. The app runs as a non-root user
and has a `HEALTHCHECK` on `/health`.

**Persistence.** The `rust_todo_pgdata` named volume is mounted at `/var/lib/postgresql/data`.
`docker compose down` keeps the data. Only `docker compose down -v` deletes it.

**Migrations.** SQL files in `migrations/` are embedded into the binary and run automatically on startup with SQLx.
They run once and are tracked in a `_sqlx_migrations` table. To add a change, create
`migrations/0002_something.sql`.

**Startup order.** The `db` service has a `pg_isready` healthcheck and the `api` service uses
`depends_on: condition: service_healthy`. The API also retries its database connection for up to 60 seconds.

**Logging.** Structured logs use the `tracing` crate. Set `RUST_LOG=debug` for more detail.

## Check persistence

```bash
curl -X POST localhost:3000/todos -H "Content-Type: application/json" -d '{"title":"survive"}'
docker compose down
docker compose up -d
curl localhost:3000/todos          # the todo is still there
```

## Check that the container is not root and is healthy

```bash
docker compose exec api id         # uid is not 0
docker ps                          # STATUS shows (healthy)
```

## Project layout

```
.
├── Cargo.toml / Cargo.lock
├── Dockerfile
├── docker-compose.yml
├── .env.example
├── .dockerignore
├── migrations/0001_create_todos.sql
├── static/index.html
└── src/main.rs
```
