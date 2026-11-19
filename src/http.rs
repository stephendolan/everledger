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
            let uri: ureq::http::Uri = url.parse().map_err(|e| {
                Failure::unreachable(format!("{}: source unreachable ({e})", request.url))
            })?;
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
            let mut response = req.call().map_err(|e| {
                Failure::unreachable(format!("{}: source unreachable ({e})", request.url))
            })?;
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
                .map_err(|e| {
                    Failure::unreachable(format!("{}: source unreachable ({e})", request.url))
                })?;
            if !(200..300).contains(&status) {
                let decoded = decode_body(&body).unwrap_or(Document::Null);
                let code = match status {
                    400 | 422 => ExitCode::Usage,
                    404 | 409 => ExitCode::Error,
                    _ => ExitCode::Unreachable,
                };
                return Err(Failure {
                    message: format!("{}: HTTP {status}", request.url),
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
    let bytes = body.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(body);
    parse(std::str::from_utf8(bytes).map_err(|_| Failure::error("invalid UTF-8 JSON"))?)
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
    document: Document,
    view: T,
}
impl<T: serde::de::DeserializeOwned> Payload<T> {
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Document>) -> Result<()> {
        let key = key.into();
        self.set_path(&[&key], value)
    }

    pub fn set_path(&mut self, path: &[&str], value: impl Into<Document>) -> Result<()> {
        self.edit(path, Some(value.into()))
    }

    pub fn remove(&mut self, key: &str) -> Result<()> {
        if self.document.get(key).is_none() {
            return Ok(());
        }
        self.edit(&[key], None)
    }

    fn edit(&mut self, path: &[&str], value: Option<Document>) -> Result<()> {
        fn parent<'a>(
            document: &'a mut Document,
            path: &[&str],
        ) -> Result<&'a mut Vec<(std::sync::Arc<str>, Document)>> {
            let mut document = document;
            for key in path {
                document = match document {
                    Document::Object(entries) => entries
                        .iter_mut()
                        .find(|(k, _)| k.as_ref() == *key)
                        .map(|(_, v)| v),
                    _ => None,
                }
                .ok_or_else(|| Failure::shape("payload update", "missing parent object"))?;
            }
            match document {
                Document::Object(entries) => Ok(entries),
                _ => Err(Failure::shape("payload update", "expected object")),
            }
        }
        let (key, ancestors) = path
            .split_last()
            .ok_or_else(|| Failure::shape("payload update", "empty path"))?;
        let entries = parent(&mut self.document, ancestors)?;
        let position = entries
            .iter()
            .position(|(k, _)| k.as_ref() == *key)
            .unwrap_or(entries.len());
        enum Undo {
            Replace(Document),
            Insert,
            Remove(Document),
        }
        let undo = match value {
            Some(value) if position < entries.len() => {
                Undo::Replace(std::mem::replace(&mut entries[position].1, value))
            }
            Some(value) => {
                entries.push(((*key).into(), value));
                Undo::Insert
            }
            None if position < entries.len() => Undo::Remove(entries.remove(position).1),
            None => return Ok(()),
        };
        match self.document.decode("payload update") {
            Ok(view) => {
                self.view = view;
                Ok(())
            }
            Err(error) => {
                let entries = parent(&mut self.document, ancestors)?;
                match undo {
                    Undo::Remove(old) => entries.insert(position, ((*key).into(), old)),
                    Undo::Replace(old) => entries[position].1 = old,
                    Undo::Insert => {
                        entries.remove(position);
                    }
                }
                Err(error)
            }
        }
    }

    pub fn set_default(
        &mut self,
        key: impl Into<String>,
        value: impl Into<Document>,
    ) -> Result<()> {
        let key = key.into();
        if self.document.get(&key).is_none() {
            self.set(key, value)?;
        }
        Ok(())
    }
    pub fn decode(document: Document, boundary: &str) -> Result<Self> {
        let view = document.decode(boundary)?;
        Ok(Self { document, view })
    }
}

impl<T> Payload<T> {
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn view(&self) -> &T {
        &self.view
    }
    pub fn into_document(self) -> Document {
        self.document
    }
}
