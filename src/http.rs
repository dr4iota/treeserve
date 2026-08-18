//! The HTTP face of treeserve: a socket, a pool of worker threads, and the
//! translation between tiny_http's types and the plain [`Req`]/[`Reply`] that
//! [`crate::handle`] speaks.
//!
//! Compiled only under the `http` feature, which is what lets the Tauri app
//! link the same router without linking a web server: it hands `handle` a
//! `Req` built from a webview request instead, over no socket at all. The
//! server is treeserve's own face and stays with treeserve — a place to add
//! authentication, TLS termination or a reverse proxy in front, none of which
//! an app talking to itself has any use for.

use std::io::Cursor;
use std::net::SocketAddr;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::hl::Hl;
use crate::{handle, Body, Config, Reply, Req, State};

/// A running server: worker threads plus the address they are serving.
pub struct Serving {
    pub addr: SocketAddr,
    pub state: Arc<State>,
    handles: Vec<JoinHandle<()>>,
}

impl Serving {
    /// Blocks until every worker thread exits (i.e. forever, in practice).
    pub fn join(self) {
        for h in self.handles {
            let _ = h.join();
        }
    }
}

/// Binds the configured address and starts the worker pool.
///
/// Pass port 0 to let the OS pick one; the assigned address is in
/// [`Serving::addr`].
pub fn spawn(cfg: Config) -> Result<Serving, Box<dyn std::error::Error + Send + Sync>> {
    let server = Server::http(format!("{}:{}", cfg.bind, cfg.port))?;
    let addr = server
        .server_addr()
        .to_ip()
        .ok_or("server is not listening on an IP address")?;

    let threads = cfg.threads;
    let hl = Hl::new(cfg.syn_light, cfg.syn_dark);
    let state = Arc::new(State { cfg, hl });
    let server = Arc::new(server);

    let mut handles = Vec::new();
    for _ in 0..threads {
        let server = Arc::clone(&server);
        let state = Arc::clone(&state);
        handles.push(thread::spawn(move || loop {
            let rq = match server.recv() {
                Ok(rq) => rq,
                Err(e) => {
                    eprintln!("recv error: {}", e);
                    break;
                }
            };
            let r = panic::catch_unwind(AssertUnwindSafe(|| respond(&state, rq)));
            if let Err(e) = r {
                eprintln!("handler panicked: {:?}", e);
            }
        }));
    }

    Ok(Serving {
        addr,
        state,
        handles,
    })
}

fn h(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("valid header")
}

/// Everything tiny_http knows that the router does not.
fn req_from(rq: &Request) -> Req {
    Req {
        url: rq.url().to_string(),
        headers: rq
            .headers()
            .iter()
            .map(|hd| (hd.field.as_str().as_str().to_string(), hd.value.as_str().to_string()))
            .collect(),
        is_get: *rq.method() == Method::Get,
    }
}

/// Writes a decided answer down the socket it came from.
fn write_reply(rq: Request, reply: Reply) {
    let headers: Vec<Header> = reply.headers.iter().map(|(k, v)| h(k, v)).collect();
    let status = StatusCode(reply.status);
    let _ = match reply.body {
        Body::Empty => rq.respond(Response::new(status, headers, std::io::empty(), Some(0), None)),
        Body::Text(body) => {
            let len = body.len();
            let cursor = Cursor::new(body.into_bytes());
            rq.respond(Response::new(status, headers, cursor, Some(len), None))
        }
        Body::Stream { reader, len } => {
            rq.respond(Response::new(status, headers, reader, Some(len as usize), None))
        }
    };
}

fn respond(state: &State, rq: Request) {
    let req = req_from(&rq);
    write_reply(rq, handle(state, &req));
}

