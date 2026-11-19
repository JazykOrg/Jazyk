//! Bounded native endpoint setup. No hosted page fetches a loopback API.
use super::state::SharedState;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Form,
};
use serde::Deserialize;
#[derive(Deserialize)]
pub struct Token {
    token: String,
}
fn allowed(st: &SharedState, token: &Token) -> bool {
    std::env::var("JAZYK_CHAT_SETUP").is_ok_and(|s| s.starts_with("endpoint"))
        && st
            .token
            .as_deref()
            .is_some_and(|expected| super::state::token_matches(Some(&token.token), expected))
}
fn html(body: &str) -> Response {
    ([("cache-control","no-store"),("referrer-policy","no-referrer"),("content-security-policy","default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'")],Html(body.to_string())).into_response()
}
pub async fn page(State(st): State<SharedState>, Query(token): Query<Token>) -> Response {
    if !allowed(&st, &token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    html(include_str!("chat_setup.html"))
}
#[derive(Deserialize)]
pub struct Endpoint {
    name: String,
    url: String,
    model: String,
    key: String,
}
pub async fn save(
    State(st): State<SharedState>,
    Query(token): Query<Token>,
    headers: HeaderMap,
    Form(body): Form<Endpoint>,
) -> Response {
    if !allowed(&st, &token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let host = headers
        .get("host")
        .and_then(|s| s.to_str().ok())
        .unwrap_or_default();
    if headers
        .get("origin")
        .and_then(|s| s.to_str().ok())
        .is_some_and(|origin| origin != format!("http://{host}"))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if body.name.is_empty()
        || body.name.len() > 64
        || !body
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return (
            StatusCode::BAD_REQUEST,
            "Use a name with letters, digits, dashes or underscores.",
        )
            .into_response();
    }
    let result = super::agents::endpoint(
        &serde_json::json!({"baseUrl":body.url,"model":body.model,"apiKey":body.key}),
        st.llm(),
    )
    .and_then(|llm| crate::chat_provider::save(&format!("endpoint:{}", body.name), &llm, None));
    match result {Ok(())=>html("<!doctype html><title>Jazyk endpoint saved</title><h1>Endpoint saved</h1><p>Return to your Jazyk conversation. Its model picker updates automatically.</p>"),Err(_)=>(StatusCode::BAD_REQUEST,"Could not save endpoint. Check the URL and model, then go back and try again.").into_response()}
}
