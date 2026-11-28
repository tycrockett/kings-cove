//! The authoritative King's Cove server: HTTP + JSON, long-polled.
//!
//! `KC_ADDR` (default `0.0.0.0:8787`) is where it listens; `KC_DATA`
//! (default `kings-cove-data.json`) is where matches are saved after every
//! change, so a restarted server resumes them. Once a second it withdraws
//! bot trade offers nobody answered in time.

use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hasher},
    io::Read,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use kings_cove_protocol::{
    ActionRequest, AddBotRequest, ConfigureRequest, CreateRequest, ErrorBody, JoinRequest, POLL_SECONDS, ReactRequest,
    SessionRequest,
};
use kings_cove_server::Store;
use serde::{Serialize, de::DeserializeOwned};
use tiny_http::{Header, Method, Request, Response, Server};

struct Shared {
    store: Mutex<Store>,
    changed: Condvar,
    data_path: String,
}

fn main() {
    let addr = std::env::var("KC_ADDR").unwrap_or_else(|_| "0.0.0.0:8787".to_owned());
    let data_path = std::env::var("KC_DATA").unwrap_or_else(|_| "kings-cove-data.json".to_owned());
    let store = match std::fs::read_to_string(&data_path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| panic!("{data_path}: {e}")),
        Err(_) => Store::new(random_seed()),
    };
    let shared = Arc::new(Shared { store: Mutex::new(store), changed: Condvar::new(), data_path });
    let ticker = shared.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
            let mut store = ticker.store.lock().expect("store lock");
            if store.expire_bot_offers(now) {
                save(&ticker, &store);
                drop(store);
                ticker.changed.notify_all();
            }
        }
    });
    let server = Server::http(&addr).unwrap_or_else(|e| panic!("listen on {addr}: {e}"));
    println!("King's Cove server listening on http://{addr}");
    for request in server.incoming_requests() {
        let shared = shared.clone();
        std::thread::spawn(move || handle(&shared, request));
    }
}

/// Server entropy: never from a client.
fn random_seed() -> u64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());
    h.finish()
}

fn handle(shared: &Shared, mut request: Request) {
    let url = request.url().to_owned();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let mut body = String::new();
    let _ = request.as_reader().take(64 * 1024).read_to_string(&mut body);
    // Browsers ask before cross-origin JSON posts: the web client is served
    // from another origin (GitHub Pages).
    if *request.method() == Method::Options {
        let _ = request.respond(with_cors(Response::from_string("").with_status_code(204)));
        return;
    }
    let result = match (request.method(), path) {
        (Method::Post, "/create") => with_body(&body, |r: CreateRequest| mutate(shared, |s| s.create(&r.name))),
        // Practice matches are for development: debug builds only.
        (Method::Post, "/practice") if cfg!(debug_assertions) => {
            with_body(&body, |r: CreateRequest| mutate(shared, |s| s.practice(&r.name)))
        }
        (Method::Post, "/configure") => with_body(&body, |r: ConfigureRequest| mutate(shared, |s| s.configure(&r))),
        (Method::Post, "/react") => {
            with_body(&body, |r: ReactRequest| mutate(shared, |s| s.react(&r.code, &r.token, r.reaction)))
        }
        (Method::Post, "/join") => with_body(&body, |r: JoinRequest| mutate(shared, |s| s.join(&r.code, &r.name))),
        (Method::Post, "/start") => {
            with_body(&body, |r: SessionRequest| mutate(shared, |s| s.start(&r.code, &r.token)))
        }
        (Method::Post, "/add-bot") => {
            with_body(&body, |r: AddBotRequest| mutate(shared, |s| s.add_bot(&r.code, &r.token, r.style)))
        }
        (Method::Post, "/action") => {
            with_body(&body, |r: ActionRequest| mutate(shared, |s| s.act(&r.code, &r.token, r.action)))
        }
        (Method::Post, "/undo") => with_body(&body, |r: SessionRequest| mutate(shared, |s| s.undo(&r.code, &r.token))),
        (Method::Post, "/end") => with_body(&body, |r: SessionRequest| mutate(shared, |s| s.end(&r.code, &r.token))),
        (Method::Get, "/poll") => poll(shared, query),
        (Method::Get, "/") => Ok("\"King's Cove\"".to_owned()),
        _ => Err((404, "Not found".to_owned())),
    };
    let json = Header::from_bytes("Content-Type", "application/json").expect("static header");
    let response = match result {
        Ok(text) => Response::from_string(text).with_header(json),
        Err((status, error)) => Response::from_string(serde_json::to_string(&ErrorBody { error }).expect("json"))
            .with_status_code(status)
            .with_header(json),
    };
    let _ = request.respond(with_cors(response));
}

/// Any origin may call: seats are authorised by the token in the body, never
/// by cookies.
fn with_cors<R: Read>(response: Response<R>) -> Response<R> {
    let header = |name: &str, value: &str| Header::from_bytes(name, value).expect("static header");
    response
        .with_header(header("Access-Control-Allow-Origin", "*"))
        .with_header(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"))
        .with_header(header("Access-Control-Allow-Headers", "Content-Type"))
        .with_header(header("Access-Control-Max-Age", "86400"))
}

type Answer = Result<String, (u16, String)>;

fn with_body<T: DeserializeOwned>(body: &str, f: impl FnOnce(T) -> Answer) -> Answer {
    let request = serde_json::from_str(body).map_err(|e| (400, format!("Bad request: {e}")))?;
    f(request)
}

/// Runs a change, saves the store and wakes the long polls.
fn mutate<T: Serialize>(shared: &Shared, f: impl FnOnce(&mut Store) -> Result<T, String>) -> Answer {
    let mut store = shared.store.lock().expect("store lock");
    let value = f(&mut store).map_err(|e| (409, e))?;
    save(shared, &store);
    drop(store);
    shared.changed.notify_all();
    Ok(serde_json::to_string(&value).expect("json"))
}

/// Writes the store aside, then renames it over the data file: a crash
/// mid-write never leaves a torn file.
fn save(shared: &Shared, store: &Store) {
    match serde_json::to_string(store) {
        Ok(saved) => {
            let tmp = format!("{}.tmp", shared.data_path);
            if let Err(e) = std::fs::write(&tmp, saved).and_then(|()| std::fs::rename(&tmp, &shared.data_path)) {
                eprintln!("save {}: {e}", shared.data_path);
            }
        }
        Err(e) => eprintln!("serialize store: {e}"),
    }
}

fn poll(shared: &Shared, query: &str) -> Answer {
    let param = |key: &str| {
        query.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v.to_owned()))
    };
    let code = param("code").unwrap_or_default().to_ascii_uppercase();
    let token = param("token").unwrap_or_default();
    let since: u64 = param("since").and_then(|s| s.parse().ok()).unwrap_or(0);
    let deadline = Instant::now() + Duration::from_secs(POLL_SECONDS);
    let mut store = shared.store.lock().expect("store lock");
    loop {
        let view = store.view(&code, &token).map_err(|e| (404, e))?;
        let now = Instant::now();
        if view.version > since || now >= deadline {
            return Ok(serde_json::to_string(&view).expect("json"));
        }
        store = shared.changed.wait_timeout(store, deadline - now).expect("store lock").0;
    }
}
