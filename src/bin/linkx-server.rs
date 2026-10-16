//! Service HTTP du tournoi : une seule route, n'importe quel chemin.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::json;
use sha2::Sha256;

use linkx_bot::notation::{format_move, parse_record};
use linkx_bot::search::Searcher;

#[derive(Deserialize)]
struct MoveRequest {
    #[serde(default)]
    game: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    record: String,
    deadline_ms: Option<u64>,
}

const MAX_CLOCK_SKEW_S: i64 = 300;

fn signature_ok(secret: &str, headers: &HeaderMap, body: &[u8]) -> bool {
    let header = |name| headers.get(name).and_then(|v| v.to_str().ok());
    let (Some(ts), Some(sig)) = (header("x-linkx-timestamp"), header("x-linkx-signature")) else {
        return false;
    };
    let Ok(ts_value) = ts.parse::<i64>() else { return false };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    if (now - ts_value).abs() > MAX_CLOCK_SKEW_S {
        return false;
    }
    let Some(Ok(expected)) = sig.strip_prefix("sha256=").map(hex::decode) else { return false };
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(ts.as_bytes());
    mac.update(b".");
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

/// Temps de réflexion : une marge fixe pour le réseau, plafonné.
fn budget(deadline_ms: u64) -> Duration {
    let margin: u64 = std::env::var("LINKX_MARGIN_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1500);
    Duration::from_millis(deadline_ms.saturating_sub(margin).clamp(100, 5000))
}

async fn play(headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    if let Ok(secret) = std::env::var("LINKX_SECRET") {
        if !secret.is_empty() && !signature_ok(&secret, &headers, &body) {
            eprintln!("signature refusée");
            return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "signature" })));
        }
    }
    let Ok(req) = serde_json::from_slice::<MoveRequest>(&body) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "json" })));
    };
    let pos = match parse_record(&req.record) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[{}] notation refusée au coup {} : {}", req.game, e.index, e.reason.code());
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": e.reason.code() })));
        }
    };
    let expected = if pos.to_move == 0 { "blue" } else { "white" };
    if !req.color.is_empty() && req.color != expected {
        eprintln!("[{}] couleur annoncée {} mais {} au trait", req.game, req.color, expected);
    }
    let time = budget(req.deadline_ms.unwrap_or(6000));
    let result = tokio::task::spawn_blocking(move || Searcher::new(22).search(&pos, time, 64))
        .await
        .ok()
        .flatten();
    match result {
        Some(r) => {
            let mv = format_move(r.best);
            eprintln!(
                "[{}] {} « {} » -> {mv} (prof. {}, score {}, {} nœuds, {} ms)",
                req.game, expected, req.record, r.depth, r.score, r.nodes, r.elapsed.as_millis()
            );
            (StatusCode::OK, Json(json!({ "move": mv })))
        }
        None => (StatusCode::CONFLICT, Json(json!({ "error": "no-move" }))),
    }
}

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .fallback(axum::routing::post(play));
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await.unwrap();
    eprintln!("linkx-bot à l'écoute sur :{port}");
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, ts: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{ts}.").as_bytes());
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    fn headers(ts: &str, sig: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-linkx-timestamp", ts.parse().unwrap());
        h.insert("x-linkx-signature", sig.parse().unwrap());
        h
    }

    #[test]
    fn signature_round_trip() {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let body = br#"{"record":""}"#;
        assert!(signature_ok("s3cret", &headers(&ts, &sign("s3cret", &ts, body)), body));
        assert!(!signature_ok("other", &headers(&ts, &sign("s3cret", &ts, body)), body));
        assert!(!signature_ok("s3cret", &headers(&ts, &sign("s3cret", &ts, body)), b"{}"));
        assert!(!signature_ok("s3cret", &headers("1000", &sign("s3cret", "1000", body)), body));
    }
}
