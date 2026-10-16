//! Service HTTP du tournoi : une seule route, n'importe quel chemin.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use tokio::sync::Semaphore;

use linkx_bot::game::Position;
use linkx_bot::notation::{format_move, parse_record_lenient};
use linkx_bot::search::{MATE, Searcher};

#[derive(Deserialize)]
struct MoveRequest {
    #[serde(default)]
    game: Option<Value>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    record: Option<String>,
    #[serde(default)]
    deadline_ms: Option<Value>,
}

const MAX_CLOCK_SKEW_S: u64 = 300;
const DEFAULT_DEADLINE_MS: u64 = 6000;
/// La plateforme n'envoie jamais plus de deux appels à la fois ; au-delà, on
/// répond vite et petit plutôt que de risquer la mémoire du conteneur.
const MAX_SEARCHES: usize = 2;
const TT_BITS: u32 = 20;
const OVERFLOW_TT_BITS: u32 = 14;

struct Config {
    secret: Option<String>,
    margin_ms: u64,
    threads: usize,
    tuning: String,
    searches: Semaphore,
    games: Games,
    ponder: bool,
}

/// Recherches gardées d'un appel à l'autre, par partie (champ `game`) : table
/// de transposition, historique et cache d'évaluation restent chauds. Le
/// protocole interdit d'en dépendre, pas d'en profiter : sans entrée, on
/// repart d'un `Searcher` neuf, comme avant.
struct Games {
    slots: Mutex<HashMap<String, GameSlot>>,
    capacity: usize,
    /// Recherches en cours pour de vrais appels : la réflexion leur cède la place.
    active: AtomicUsize,
    /// Arrêt du service : plus aucune réflexion, pour ne pas retarder la sortie.
    closing: AtomicBool,
}

/// Compte une vraie recherche dans `active`, décomptée même en cas de panique.
struct ActiveSearch<'a>(&'a AtomicUsize);

impl<'a> ActiveSearch<'a> {
    fn enter(active: &'a AtomicUsize) -> Self {
        active.fetch_add(1, SeqCst);
        ActiveSearch(active)
    }
}

impl Drop for ActiveSearch<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, SeqCst);
    }
}

struct GameSlot {
    searcher: Arc<Mutex<Searcher>>,
    stop: Arc<AtomicBool>,
    /// Levé pendant une tranche de réflexion : seule celle-ci peut être
    /// interrompue par l'appel d'une autre partie, jamais une vraie recherche.
    pondering: Arc<AtomicBool>,
    last_used: Instant,
}

#[derive(Clone)]
struct Slot {
    searcher: Arc<Mutex<Searcher>>,
    stop: Arc<AtomicBool>,
    pondering: Arc<AtomicBool>,
}

/// Tranche de réflexion : entre deux tranches, on vérifie qu'aucun appel n'attend.
const PONDER_SLICE: Duration = Duration::from_millis(300);
const PONDER_MAX: Duration = Duration::from_secs(15);
/// Profondeur maximale d'une tranche de réflexion.
const PONDER_DEPTH: u32 = 64;

impl Games {
    fn new(capacity: usize) -> Self {
        Games {
            slots: Mutex::new(HashMap::new()),
            capacity,
            active: AtomicUsize::new(0),
            closing: AtomicBool::new(false),
        }
    }

    /// Une vraie recherche ou l'arrêt du service attend : la réflexion s'efface.
    fn busy(&self) -> bool {
        self.active.load(SeqCst) != 0 || self.closing.load(SeqCst)
    }

    /// Arrêt du service : les réflexions en cours s'interrompent aussitôt.
    fn close(&self) {
        self.closing.store(true, SeqCst);
        self.stop_ponders();
    }

    fn slots(&self) -> MutexGuard<'_, HashMap<String, GameSlot>> {
        self.slots.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Le `Searcher` de cette partie, créé au besoin, la moins récente évincée.
    fn slot(&self, game: &str, tuning: &str, threads: usize) -> Option<Slot> {
        if game.is_empty() || self.capacity == 0 {
            return None;
        }
        let mut slots = self.slots();
        if let Some(slot) = slots.get_mut(game) {
            slot.last_used = Instant::now();
            return Some(Slot {
                searcher: slot.searcher.clone(),
                stop: slot.stop.clone(),
                pondering: slot.pondering.clone(),
            });
        }
        // Évincer une entrée occupée est sans danger : qui la tient garde son
        // `Arc`, et une réflexion s'arrête dès qu'un vrai appel est en cours.
        if slots.len() >= self.capacity
            && let Some(oldest) = slots
                .iter()
                .min_by_key(|(_, s)| s.last_used)
                .map(|(k, _)| k.clone())
        {
            slots.remove(&oldest);
        }
        let mut searcher = Searcher::new(TT_BITS);
        searcher.configure(tuning);
        searcher.options.threads = threads;
        let slot = Slot {
            stop: searcher.stop_handle(),
            searcher: Arc::new(Mutex::new(searcher)),
            pondering: Arc::new(AtomicBool::new(false)),
        };
        slots.insert(
            game.to_string(),
            GameSlot {
                searcher: slot.searcher.clone(),
                stop: slot.stop.clone(),
                pondering: slot.pondering.clone(),
                last_used: Instant::now(),
            },
        );
        Some(slot)
    }

    /// Interrompt les réflexions en cours, et elles seules.
    fn stop_ponders(&self) {
        for slot in self.slots().values() {
            if slot.pondering.load(SeqCst) {
                slot.stop.store(true, SeqCst);
            }
        }
    }
}

/// Reprend la main sur un `Searcher` en train de réfléchir : `search` remet le
/// drapeau à zéro en démarrant, on le relève jusqu'à obtenir le verrou.
fn take<'a>(searcher: &'a Mutex<Searcher>, stop: &AtomicBool) -> MutexGuard<'a, Searcher> {
    loop {
        stop.store(true, SeqCst);
        match searcher.try_lock() {
            Ok(guard) => return guard,
            Err(TryLockError::Poisoned(p)) => return p.into_inner(),
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}

/// Réflexion pendant le tour adverse, sur la position qui suit notre coup.
fn ponder(config: &Config, slot: &Slot, after: Position) {
    let started = Instant::now();
    while started.elapsed() < PONDER_MAX && !config.games.busy() {
        let Ok(mut guard) = slot.searcher.try_lock() else {
            return;
        };
        slot.pondering.store(true, SeqCst);
        // Gain immédiat, mat trouvé ou profondeur maximale : une tranche de
        // plus n'apprendrait rien et tournerait à vide.
        let done = config.games.busy()
            || guard
                .search(&after, PONDER_SLICE, PONDER_DEPTH)
                .is_none_or(|r| {
                    r.score.abs() >= MATE - PONDER_DEPTH as i32 || r.depth >= PONDER_DEPTH
                });
        slot.pondering.store(false, SeqCst);
        if done {
            return;
        }
    }
}

fn env_number(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

impl Config {
    fn from_env() -> Self {
        Config {
            secret: std::env::var("LINKX_SECRET").ok().filter(|s| !s.is_empty()),
            margin_ms: env_number("LINKX_MARGIN_MS", 1500),
            threads: env_number("LINKX_THREADS", 1).max(1) as usize,
            tuning: std::env::var("LINKX_CONFIG").unwrap_or_default(),
            searches: Semaphore::new(MAX_SEARCHES),
            games: Games::new(env_number("LINKX_GAMES", 3) as usize),
            ponder: env_number("LINKX_PONDER", 1) != 0,
        }
    }

    /// Temps de réflexion : le délai annoncé moins une marge pour le réseau.
    fn budget(&self, deadline_ms: u64) -> Duration {
        Duration::from_millis(deadline_ms.saturating_sub(self.margin_ms).clamp(100, 5000))
    }
}

fn signature_ok(secret: &str, headers: &HeaderMap, body: &[u8]) -> bool {
    let header = |name| headers.get(name).and_then(|v| v.to_str().ok());
    let (Some(ts), Some(sig)) = (header("x-linkx-timestamp"), header("x-linkx-signature")) else {
        return false;
    };
    let Ok(ts_value) = ts.parse::<u64>() else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    if now.abs_diff(ts_value) > MAX_CLOCK_SKEW_S {
        return false;
    }
    let Some(Ok(expected)) = sig.strip_prefix("sha256=").map(hex::decode) else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(ts.as_bytes());
    mac.update(b".");
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

/// Un délai illisible, négatif ou flottant ne doit jamais faire échouer l'appel.
fn deadline_ms(value: Option<&Value>) -> u64 {
    value
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v > 0.0)
        .map_or(DEFAULT_DEADLINE_MS, |v| v.min(60_000.0) as u64)
}

fn reply(status: StatusCode, body: Value) -> (StatusCode, Json<Value>) {
    (status, Json(body))
}

async fn play(
    State(config): State<Arc<Config>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let started = Instant::now();
    if let Some(secret) = &config.secret
        && !signature_ok(secret, &headers, &body)
    {
        eprintln!("signature refusée");
        return reply(StatusCode::UNAUTHORIZED, json!({ "error": "signature" }));
    }
    let Ok(req) = serde_json::from_slice::<MoveRequest>(&body) else {
        return reply(StatusCode::BAD_REQUEST, json!({ "error": "json" }));
    };
    let game = match req.game {
        Some(Value::String(g)) => g,
        Some(other) => other.to_string(),
        None => String::new(),
    };
    let record = req.record.unwrap_or_default();
    let pos = match parse_record_lenient(&record) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "[{game}] notation refusée au coup {} : {}",
                e.index,
                e.reason.code()
            );
            return reply(StatusCode::BAD_REQUEST, json!({ "error": e.reason.code() }));
        }
    };
    let mut legal = Vec::new();
    pos.legal_moves(&mut legal);
    let Some(&fallback) = legal.first() else {
        return reply(StatusCode::CONFLICT, json!({ "error": "no-move" }));
    };
    let to_move = if pos.to_move == 0 { "blue" } else { "white" };
    if let Some(color) = req.color.filter(|c| c != to_move) {
        eprintln!("[{game}] couleur annoncée {color} mais {to_move} au trait");
    }

    let deadline = Duration::from_millis(deadline_ms(req.deadline_ms.as_ref()));
    let budget = config.budget(deadline.as_millis() as u64);
    // Filet ultime : même si la recherche dérape, on répond avant le délai.
    let hard_stop = deadline.saturating_sub(Duration::from_millis(config.margin_ms / 2));
    let search_deadline = started + budget;
    let outcome = tokio::time::timeout(hard_stop.saturating_sub(started.elapsed()), async {
        let permit = tokio::time::timeout(budget / 2, config.searches.acquire()).await;
        let permitted = matches!(permit, Ok(Ok(_)));
        let config = config.clone();
        let game = game.clone();
        let search = tokio::task::spawn_blocking(move || {
            let _active = ActiveSearch::enter(&config.games.active);
            config.games.stop_ponders();
            let slot = if permitted {
                config.games.slot(&game, &config.tuning, config.threads)
            } else {
                None
            };
            let result = match &slot {
                Some(slot) => {
                    let mut guard = take(&slot.searcher, &slot.stop);
                    let time = search_deadline.saturating_duration_since(Instant::now());
                    guard.search(&pos, time.max(Duration::from_millis(50)), 64)
                }
                None => {
                    let (tt_bits, time) = if permitted {
                        (
                            TT_BITS,
                            search_deadline.saturating_duration_since(Instant::now()),
                        )
                    } else {
                        (OVERFLOW_TT_BITS, budget / 10)
                    };
                    let mut searcher = Searcher::new(tt_bits);
                    searcher.configure(&config.tuning);
                    searcher.options.threads = config.threads;
                    searcher.search(&pos, time, 64)
                }
            };
            (result, slot)
        });
        let result = search.await;
        drop(permit);
        result
    })
    .await;

    let (outcome, cached) = match outcome {
        Ok(Ok((r, cached))) => (Ok(Ok(r)), cached),
        Ok(Err(e)) => (Ok(Err(e)), None),
        Err(e) => (Err(e), None),
    };
    if let (true, Some(slot), Ok(Ok(Some(r)))) = (config.ponder, cached, &outcome) {
        let mut after = pos;
        if after.play(r.best).is_ok() && after.outcome.is_none() {
            let config = config.clone();
            tokio::task::spawn_blocking(move || ponder(&config, &slot, after));
        }
    }
    let (mv, detail) = match outcome {
        Ok(Ok(Some(r))) => (
            r.best,
            format!("prof. {}, score {}, {} nœuds", r.depth, r.score, r.nodes),
        ),
        Ok(Ok(None)) => (fallback, "aucun résultat, coup de repli".into()),
        Ok(Err(e)) => (fallback, format!("recherche en échec ({e}), coup de repli")),
        Err(_) => (fallback, "délai dépassé, coup de repli".into()),
    };
    let mv = format_move(mv);
    eprintln!(
        "[{game}] {to_move} « {record} » -> {mv} ({detail}, {} ms)",
        started.elapsed().as_millis()
    );
    reply(StatusCode::OK, json!({ "move": mv }))
}

fn app(config: Arc<Config>) -> Router {
    Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .fallback(axum::routing::post(play))
        .with_state(config)
}

/// PID 1 d'un conteneur : sans gestionnaire, SIGTERM est ignoré.
async fn shutdown() {
    let mut term =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
    tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

#[tokio::main]
async fn main() {
    let config = Arc::new(Config::from_env());
    // Un réglage invalide doit faire échouer le démarrage, pas une partie.
    Searcher::new(4).configure(&config.tuning);
    if config.secret.is_none() {
        eprintln!("LINKX_SECRET absent : les signatures ne sont pas vérifiées");
    }
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap();
    eprintln!("Promptoramix à l'écoute sur :{port}");
    axum::serve(listener, app(config.clone()))
        .with_graceful_shutdown(shutdown())
        .await
        .unwrap();
    // Sans cela, l'arrêt du runtime attendrait la fin des réflexions (`PONDER_MAX`).
    config.games.close();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

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

    fn now() -> String {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string()
    }

    fn config(secret: Option<&str>) -> Arc<Config> {
        Arc::new(Config {
            secret: secret.map(str::to_string),
            margin_ms: 1500,
            threads: 1,
            tuning: String::new(),
            searches: Semaphore::new(MAX_SEARCHES),
            games: Games::new(3),
            // Une réflexion en cours retarde la fin du test jusqu'à `PONDER_MAX`.
            ponder: false,
        })
    }

    async fn post(config: Arc<Config>, body: &str, headers: HeaderMap) -> (StatusCode, Value) {
        let mut req = Request::post("/coup")
            .body(Body::from(body.to_string()))
            .unwrap();
        *req.headers_mut() = headers;
        let res = app(config).oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), 1 << 16).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn assert_legal(record: &str, mv: &Value) {
        let mv = mv["move"].as_str().expect("un coup");
        let mut pos = parse_record_lenient(record).unwrap();
        pos.play(linkx_bot::notation::parse_move(mv).unwrap())
            .unwrap();
    }

    #[test]
    fn signature_round_trip() {
        let ts = now();
        let body = br#"{"record":""}"#;
        assert!(signature_ok(
            "s3cret",
            &headers(&ts, &sign("s3cret", &ts, body)),
            body
        ));
        assert!(!signature_ok(
            "other",
            &headers(&ts, &sign("s3cret", &ts, body)),
            body
        ));
        assert!(!signature_ok(
            "s3cret",
            &headers(&ts, &sign("s3cret", &ts, body)),
            b"{}"
        ));
        assert!(!signature_ok(
            "s3cret",
            &headers("1000", &sign("s3cret", "1000", body)),
            body
        ));
        assert!(!signature_ok(
            "s3cret",
            &headers("-5", &sign("s3cret", "-5", body)),
            body
        ));
    }

    #[test]
    fn odd_deadlines_fall_back_or_clamp() {
        assert_eq!(deadline_ms(None), DEFAULT_DEADLINE_MS);
        assert_eq!(deadline_ms(Some(&json!(6000.0))), 6000);
        assert_eq!(deadline_ms(Some(&json!(-3))), DEFAULT_DEADLINE_MS);
        assert_eq!(deadline_ms(Some(&json!("6000"))), DEFAULT_DEADLINE_MS);
        assert_eq!(deadline_ms(Some(&json!(1e12))), 60_000);
    }

    #[tokio::test]
    async fn signed_call_gets_a_legal_move_in_time() {
        let body =
            r#"{"protocol":1,"game":"t","color":"white","record":"b 4Lr35","deadline_ms":2000}"#;
        let ts = now();
        let started = Instant::now();
        let (status, json) = post(
            config(Some("k")),
            body,
            headers(&ts, &sign("k", &ts, body.as_bytes())),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(started.elapsed() < Duration::from_millis(2000));
        assert_legal("b 4Lr35", &json);
    }

    #[tokio::test]
    async fn pondering_never_delays_the_next_call() {
        let config = Arc::new(Config {
            ponder: true,
            ..Arc::into_inner(config(None)).unwrap()
        });
        let (_, first) = post(
            config.clone(),
            r#"{"game":"p","record":"4Lr35","deadline_ms":2000}"#,
            HeaderMap::new(),
        )
        .await;
        let ours = first["move"].as_str().unwrap().to_string();
        // La réflexion tourne maintenant sur la position après notre coup.
        tokio::time::sleep(Duration::from_millis(400)).await;
        let mut pos = parse_record_lenient(&format!("4Lr35 {ours}")).unwrap();
        let mut replies = Vec::new();
        pos.legal_moves(&mut replies);
        pos.play(replies[0]).unwrap();
        let record = format!(
            "4Lr35 {ours} {}",
            linkx_bot::notation::format_move(replies[0])
        );
        let body = format!(r#"{{"game":"p","record":"{record}","deadline_ms":2000}}"#);
        let started = Instant::now();
        let (status, second) = post(config.clone(), &body, HeaderMap::new()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            started.elapsed() < Duration::from_millis(2000),
            "{:?}",
            started.elapsed()
        );
        assert_legal(&record, &second);
        assert_eq!(config.games.slots().len(), 1);
        config.games.close();
    }

    #[tokio::test]
    async fn bad_signature_is_refused() {
        let body = r#"{"record":""}"#;
        let (status, _) = post(config(Some("k")), body, headers(&now(), "sha256=00")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn sloppy_requests_still_get_a_move() {
        for (body, record) in [
            (r#"{"record":null,"deadline_ms":1800.5}"#, ""),
            (
                r#"{"protocol":99,"game":42,"record":"15,--+16","deadline_ms":-1}"#,
                "15 16",
            ),
            (r#"{"record":"4Tr24"}"#, "4Tr24"),
        ] {
            let (status, json) = post(config(None), body, HeaderMap::new()).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_legal(record, &json);
        }
    }

    #[tokio::test]
    async fn unreadable_record_is_a_client_error() {
        let (status, json) = post(
            config(None),
            r#"{"record":"15 15 15 15 15"}"#,
            HeaderMap::new(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"], "exhausted");
    }

    #[tokio::test]
    async fn concurrent_calls_beyond_the_limit_still_answer() {
        let config = config(None);
        let body = r#"{"record":"4Lr35","deadline_ms":2500}"#;
        let calls: Vec<_> = (0..4)
            .map(|_| tokio::spawn(post(config.clone(), body, HeaderMap::new())))
            .collect();
        for call in calls {
            let (status, json) = call.await.unwrap();
            assert_eq!(status, StatusCode::OK);
            assert_legal("4Lr35", &json);
        }
    }
}
