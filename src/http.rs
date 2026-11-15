#[path = "http_forward.rs"]
mod forward;
use crate::{
    ExitCode, Failure, Result,
    document::{Document, parse},
};
use std::{collections::HashMap, time::Duration};

#[derive(Clone, Debug)]
pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub redirects: Redirects,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redirects {
    Follow,
    Refuse,
}
impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: vec![],
            redirects: Redirects::Follow,
        }
    }
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}
#[derive(Clone, Debug)]
pub struct Response {
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}
impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}
pub trait Http {
    fn request(&self, request: &Request) -> Result<Response>;
}
#[derive(Default)]
pub struct UreqHttp;
fn env_proxy(scheme: &str, host: &str, port: Option<u16>) -> Result<Option<ureq::Proxy>> {
    let var = |name: &str| {
        std::env::var(name.to_ascii_lowercase())
            .or_else(|_| std::env::var(name.to_ascii_uppercase()))
            .ok()
    };
    let no_proxy = var("no_proxy").unwrap_or_default();
    let host = host.to_ascii_lowercase();
    let hostport = port.map_or_else(|| host.clone(), |p| format!("{host}:{p}"));
    if no_proxy.split(',').any(|entry| {
        let entry = entry.trim().trim_start_matches('.').to_ascii_lowercase();
        entry == "*"
            || (!entry.is_empty()
                && [&host, &hostport]
                    .iter()
                    .any(|h| **h == entry || h.ends_with(&format!(".{entry}"))))
    }) {
        return Ok(None);
    }
    let value = if scheme == "http" && std::env::var_os("REQUEST_METHOD").is_some() {
        std::env::var("http_proxy").ok()
    } else {
        var(&format!("{scheme}_proxy"))
    };
    value
        .filter(|s| !s.is_empty())
        .map(|s| {
            ureq::Proxy::new(&s).map_err(|e| Failure::unreachable(format!("invalid proxy: {e}")))
        })
        .transpose()
}
impl Http for UreqHttp {
    fn request(&self, request: &Request) -> Result<Response> {
        let mut url = request.url.clone();
        #[cfg(feature = "parity-seams")]
        {
            let uri: ureq::http::Uri = url
                .parse()
                .map_err(|e| Failure::unreachable(format!("invalid URL: {e}")))?;
            if uri.host() == Some("foreverdb.net") {
                let base = std::env::var("EVERLEDGER_SEAM_FOREVERDB_URL").map_err(|_| {
                    Failure::unreachable("ForeverDB disabled without EVERLEDGER_SEAM_FOREVERDB_URL")
                })?;
                url = format!(
                    "{}{}",
                    base.trim_end_matches('/'),
                    uri.path_and_query().map_or("/", |p| p.as_str())
                );
            }
        }
        let mut visits = HashMap::<String, u32>::new();
        for redirect in 0..=10 {
            let uri: ureq::http::Uri = url
                .parse()
                .map_err(|e| Failure::unreachable(format!("{url}: source unreachable ({e})")))?;
            let proxy = env_proxy(
                uri.scheme_str().unwrap_or("http"),
                uri.host().unwrap_or(""),
                uri.port_u16(),
            )?;
            let forward = uri.scheme_str() == Some("http")
                && proxy.as_ref().is_some_and(|p| {
                    matches!(
                        p.protocol(),
                        ureq::ProxyProtocol::Http | ureq::ProxyProtocol::Https
                    )
                });
            let config = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .max_redirects(0)
                .timeout_connect(Some(Duration::from_secs(20)))
                .timeout_per_read(Some(Duration::from_secs(20)))
                .timeout_per_write(Some(Duration::from_secs(20)))
                .tls_config(
                    ureq::tls::TlsConfig::builder()
                        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                        .build(),
                )
                .proxy(if forward { None } else { proxy.clone() })
                .build();
            let agent: ureq::Agent = if forward {
                forward::agent(config, proxy.as_ref().unwrap())
            } else {
                config.into()
            };
            let mut req = agent.get(&url);
            for (name, value) in [
                ("User-Agent", "everledger"),
                ("Accept-Encoding", "identity"),
                ("Connection", "close"),
            ] {
                if !request
                    .headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case(name))
                {
                    req = req.header(name, value);
                }
            }
            if forward
                && let Some(proxy) = &proxy
                && let Some(user) = proxy.username()
            {
                let auth = format!("{user}:{}", proxy.password().unwrap_or(""));
                let encoded = crate::primitives::base64_encode(auth.as_bytes())
                    .replace('-', "+")
                    .replace('_', "/");
                req = req.header("Proxy-Authorization", format!("Basic {encoded}"));
            }
            for (name, value) in &request.headers {
                req = req.header(name, value);
            }
            let mut response = req
                .call()
                .map_err(|e| Failure::unreachable(format!("{url}: source unreachable ({e})")))?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308)
                && request.redirects == Redirects::Follow
                && let Some(location) = response
                    .headers()
                    .get("location")
                    .and_then(|h| h.to_str().ok())
            {
                let next = resolve_url(&url, location)?;
                let count = visits.entry(next.clone()).or_default();
                *count += 1;
                if redirect < 10 && *count < 4 {
                    url = next;
                    continue;
                }
            }
            let headers = response
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").into()))
                .collect();
            let body = response
                .body_mut()
                .with_config()
                .limit(u64::MAX)
                .read_to_vec()
                .map_err(|e| Failure::unreachable(format!("{url}: source unreachable ({e})")))?;
            if !(200..300).contains(&status) {
                let decoded = decode_body(&body).unwrap_or(Document::Null);
                let code = match status {
                    400 | 422 => ExitCode::Usage,
                    404 | 409 => ExitCode::Error,
                    _ => ExitCode::Unreachable,
                };
                return Err(Failure {
                    message: format!("{url}: HTTP {status}"),
                    code,
                    body: Some(Document::object([
                        ("status", Document::from(i64::from(status))),
                        ("response", decoded),
                    ])),
                });
            }
            return Ok(Response { body, headers });
        }
        unreachable!("redirect loop returns an HTTP error")
    }
}
pub fn decode_body(body: &[u8]) -> Result<Document> {
    enum Encoding {
        Utf8,
        Utf16(bool),
        Utf32(bool),
    }
    let (encoding, body) = if body.starts_with(&[0xff, 0xfe, 0, 0]) {
        (Encoding::Utf32(true), &body[4..])
    } else if body.starts_with(&[0, 0, 0xfe, 0xff]) {
        (Encoding::Utf32(false), &body[4..])
    } else if body.starts_with(&[0xff, 0xfe]) {
        (Encoding::Utf16(true), &body[2..])
    } else if body.starts_with(&[0xfe, 0xff]) {
        (Encoding::Utf16(false), &body[2..])
    } else if body.starts_with(&[0xef, 0xbb, 0xbf]) {
        (Encoding::Utf8, &body[3..])
    } else if body.len() >= 4 && body[0] == 0 {
        (
            if body[1] == 0 {
                Encoding::Utf32(false)
            } else {
                Encoding::Utf16(false)
            },
            body,
        )
    } else if body.len() >= 4 && body[1] == 0 {
        (
            if body[2] == 0 && body[3] == 0 {
                Encoding::Utf32(true)
            } else {
                Encoding::Utf16(true)
            },
            body,
        )
    } else if body.len() == 2 && body[0] == 0 {
        (Encoding::Utf16(false), body)
    } else if body.len() == 2 && body[1] == 0 {
        (Encoding::Utf16(true), body)
    } else {
        (Encoding::Utf8, body)
    };
    let invalid = || Failure::error("invalid JSON character encoding");
    let text = match encoding {
        Encoding::Utf8 => std::str::from_utf8(body).map_err(|_| invalid())?.to_owned(),
        Encoding::Utf16(little) => {
            if !body.len().is_multiple_of(2) {
                return Err(invalid());
            }
            let units = body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&b| {
                    if little {
                        u16::from_le_bytes(b)
                    } else {
                        u16::from_be_bytes(b)
                    }
                })
                .collect::<Vec<_>>();
            String::from_utf16(&units).map_err(|_| invalid())?
        }
        Encoding::Utf32(little) => {
            if !body.len().is_multiple_of(4) {
                return Err(invalid());
            }
            body.as_chunks::<4>()
                .0
                .iter()
                .map(|&b| {
                    char::from_u32(if little {
                        u32::from_le_bytes(b)
                    } else {
                        u32::from_be_bytes(b)
                    })
                    .ok_or_else(invalid)
                })
                .collect::<Result<String>>()?
        }
    };
    parse(&text)
}

fn resolve_url(base: &str, location: &str) -> Result<String> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return Ok(location.into());
    }
    let uri: ureq::http::Uri = base
        .parse()
        .map_err(|e| Failure::error(format!("invalid redirect URL: {e}")))?;
    let scheme = uri.scheme_str().unwrap_or("http");
    let auth = uri
        .authority()
        .ok_or_else(|| Failure::error("invalid redirect authority"))?;
    if location.starts_with("//") {
        return Ok(format!("{scheme}:{location}"));
    }
    let path = if location.starts_with('/') {
        location.into()
    } else if location.starts_with('?') {
        format!("{}{location}", uri.path())
    } else {
        format!(
            "{}/{location}",
            uri.path().rsplit_once('/').map_or("", |(p, _)| p)
        )
    };
    let mut pieces = vec![];
    for p in path.split('/') {
        match p {
            ".." => {
                pieces.pop();
            }
            "." => {}
            _ => pieces.push(p),
        }
    }
    Ok(format!("{scheme}://{auth}{}", pieces.join("/")))
}
#[derive(Clone, Debug)]
pub struct Payload<T> {
    pub document: Document,
    pub view: T,
}
impl<T: serde::de::DeserializeOwned> Payload<T> {
    pub fn decode(document: Document, boundary: &str) -> Result<Self> {
        let view = document.decode(boundary)?;
        Ok(Self { document, view })
    }
}
