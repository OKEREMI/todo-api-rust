use std::{env, time::Duration};

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, FromRow, PgPool};
use tracing::{error, info, warn};

#[derive(Debug, Serialize, FromRow)]
struct Todo {
    id: i64,
    title: String,
    completed: bool,
}

#[derive(Debug, Deserialize)]
struct CreateTodo {
    title: Option<String>,
    completed: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpdateTodo {
    title: Option<String>,
    completed: Option<bool>,
}

// One error type so every failure becomes a clean JSON response, never a crash.
enum ApiError {
    BadRequest(String),
    NotFound,
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "todo not found".to_string()),
            ApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error".to_string(),
            ),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        error!("database error: {e}");
        ApiError::Internal
    }
}

impl From<JsonRejection> for ApiError {
    fn from(_: JsonRejection) -> Self {
        ApiError::BadRequest("request body must be valid JSON".to_string())
    }
}

// Turns a bad :id such as "abc" into a 400 instead of Axum's default plain text.
fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| ApiError::BadRequest("id must be a positive integer".to_string()))
}

fn clean_title(title: Option<String>) -> Result<String, ApiError> {
    match title.map(|t| t.trim().to_string()) {
        Some(t) if !t.is_empty() => Ok(t),
        _ => Err(ApiError::BadRequest(
            "title is required and cannot be empty".to_string(),
        )),
    }
}

// The frontend is compiled into the binary, so no extra files or containers are needed.
async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn health(State(pool): State<PgPool>) -> Result<Json<serde_json::Value>, ApiError> {
    sqlx::query("SELECT 1").execute(&pool).await?;
    Ok(Json(json!({ "status": "ok" })))
}

async fn list_todos(State(pool): State<PgPool>) -> Result<Json<Vec<Todo>>, ApiError> {
    let todos = sqlx::query_as::<_, Todo>("SELECT id, title, completed FROM todos ORDER BY id")
        .fetch_all(&pool)
        .await?;
    Ok(Json(todos))
}

async fn create_todo(
    State(pool): State<PgPool>,
    body: Result<Json<CreateTodo>, JsonRejection>,
) -> Result<(StatusCode, Json<Todo>), ApiError> {
    let Json(input) = body?;
    let title = clean_title(input.title)?;
    let todo = sqlx::query_as::<_, Todo>(
        "INSERT INTO todos (title, completed) VALUES ($1, $2) RETURNING id, title, completed",
    )
    .bind(title)
    .bind(input.completed.unwrap_or(false))
    .fetch_one(&pool)
    .await?;
    Ok((StatusCode::CREATED, Json(todo)))
}

async fn get_todo(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<Json<Todo>, ApiError> {
    let id = parse_id(&id)?;
    let todo = sqlx::query_as::<_, Todo>("SELECT id, title, completed FROM todos WHERE id = $1")
        .bind(id)
        .fetch_optional(&pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(todo))
}

async fn update_todo(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
    body: Result<Json<UpdateTodo>, JsonRejection>,
) -> Result<Json<Todo>, ApiError> {
    let id = parse_id(&id)?;
    let Json(input) = body?;
    let title = match input.title {
        Some(t) => Some(clean_title(Some(t))?),
        None => None,
    };
    // COALESCE keeps the old value when a field is not sent.
    let todo = sqlx::query_as::<_, Todo>(
        "UPDATE todos SET title = COALESCE($1, title), completed = COALESCE($2, completed) \
         WHERE id = $3 RETURNING id, title, completed",
    )
    .bind(title)
    .bind(input.completed)
    .bind(id)
    .fetch_optional(&pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(todo))
}

async fn delete_todo(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    let result = sqlx::query("DELETE FROM todos WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn not_found() -> ApiError {
    ApiError::NotFound
}

// depends_on only waits for the container to start, so we also retry here.
async fn connect_with_retry(url: &str) -> PgPool {
    for attempt in 1..=30 {
        match PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await
        {
            Ok(pool) => return pool,
            Err(e) => {
                warn!("database not ready (attempt {attempt}/30): {e}");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    panic!("could not connect to the database after 30 attempts");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Nothing is hardcoded: the connection string comes from the environment.
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let port = env::var("PORT").unwrap_or_else(|_| "3000".to_string());

    let pool = connect_with_retry(&database_url).await;
    info!("connected to database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");
    info!("migrations applied");

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/todos", get(list_todos).post(create_todo))
        .route(
            "/todos/:id",
            get(get_todo).put(update_todo).delete(delete_todo),
        )
        .fallback(not_found)
        .with_state(pool);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind port");
    info!("listening on {addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}
