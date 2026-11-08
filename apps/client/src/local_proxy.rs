use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use subtle::ConstantTimeEq;
use tiny_http::{Header, Response, Server, StatusCode};
pub struct Proxy {
    pub base: String,
    pub token: String,
    stop: Arc<AtomicBool>,
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
impl Proxy {
    pub fn start(base: &str, key: &str) -> Result<Self, String> {
        let server = Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = server.server_addr().to_ip().ok_or("本地端口不可用")?.port();
        use rand::RngCore;
        let mut b = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut b);
        let token: String = b.iter().map(|b| format!("{b:02x}")).collect();
        let local = Self {
            base: format!("http://127.0.0.1:{port}/v1"),
            token: token.clone(),
            stop: Arc::new(AtomicBool::new(false)),
        };
        let stop = local.stop.clone();
        let base = base.to_owned();
        let key = key.to_owned();
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(180))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            let active = Arc::new(AtomicUsize::new(0));
            while !stop.load(Ordering::Relaxed) {
                let Ok(Some(mut request)) = server.recv_timeout(Duration::from_millis(200)) else {
                    continue;
                };
                let expected = format!("Bearer {token}");
                let got = request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Authorization"))
                    .map(|h| h.value.as_str())
                    .unwrap_or("");
                if !bool::from(expected.as_bytes().ct_eq(got.as_bytes())) {
                    let _ = request.respond(Response::empty(StatusCode(401)));
                    continue;
                }
                if !matches!(
                    request.url(),
                    "/v1/chat/completions"
                        | "/v1/responses"
                        | "/v1/messages"
                        | "/v1/models"
                        | "/v1/embeddings"
                        | "/v1/completions"
                ) {
                    let _ = request.respond(Response::empty(StatusCode(404)));
                    continue;
                }
                // 方法白名单:仅模型列表允许 GET,其余只允许 POST
                let method_ok = if request.url() == "/v1/models" {
                    request.method().as_str() == "GET"
                } else {
                    request.method().as_str() == "POST"
                };
                if !method_ok {
                    let _ = request.respond(Response::empty(StatusCode(405)));
                    continue;
                }
                if active.load(Ordering::Relaxed) >= 8 {
                    let _ = request.respond(Response::empty(StatusCode(429)));
                    continue;
                }
                let mut body = Vec::new();
                use std::io::Read;
                if request
                    .as_reader()
                    .take(16 * 1024 * 1024 + 1)
                    .read_to_end(&mut body)
                    .is_err()
                    || body.len() > 16 * 1024 * 1024
                {
                    let _ = request.respond(Response::empty(StatusCode(413)));
                    continue;
                }
                let http = http.clone();
                let url = format!(
                    "{}{}",
                    base.trim_end_matches("/v1").trim_end_matches('/'),
                    request.url()
                );
                let key = key.clone();
                let active = active.clone();
                active.fetch_add(1, Ordering::Relaxed);
                std::thread::spawn(move || {
                    let method = reqwest::Method::from_bytes(request.method().as_str().as_bytes())
                        .unwrap_or(reqwest::Method::POST);
                    let result = http
                        .request(method, url)
                        .bearer_auth(key)
                        .header("X-01Code-Client", crate::gateway::CLIENT)
                        .header("Content-Type", "application/json")
                        .body(body)
                        .send();
                    match result {
                        Ok(up) => {
                            let status = StatusCode(up.status().as_u16());
                            let headers = up
                                .headers()
                                .iter()
                                .filter(|(k, _)| {
                                    matches!(k.as_str(), "content-type" | "retry-after")
                                })
                                .filter_map(|(k, v)| {
                                    Header::from_bytes(k.as_str(), v.as_bytes()).ok()
                                })
                                .collect();
                            let _ = request.respond(Response::new(status, headers, up, None, None));
                        }
                        Err(_) => {
                            let _ = request.respond(
                                Response::from_string("Gateway unavailable")
                                    .with_status_code(StatusCode(502)),
                            );
                        }
                    }
                    active.fetch_sub(1, Ordering::Relaxed);
                });
            }
        });
        Ok(local)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_proxy_requires_secret_and_restricts_paths() {
        let p = Proxy::start("https://example.invalid/v1", "secret").unwrap();
        let h = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap();
        assert_eq!(
            h.get(format!("{}/models", p.base)).send().unwrap().status(),
            401
        );
        assert_eq!(
            h.get(format!("{}/../api/admin", p.base))
                .bearer_auth(&p.token)
                .send()
                .unwrap()
                .status(),
            404
        );
    }
}
