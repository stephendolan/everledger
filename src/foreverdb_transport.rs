use crate::{
    Context, Failure, Result,
    document::Document,
    http::{Redirects, Request},
};
use serde::Deserialize;
use std::process::{Command, Stdio};
pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
/// ```compile_fail
/// fn log_key(key: &everledger::foreverdb::transport::ApiKey) {
///     let _ = format!("{key:?}");
/// }
/// ```
#[derive(Clone)]
pub struct ApiKey(String);
#[derive(Clone)]
pub enum Transport {
    EnvKey(ApiKey),
    ConnectorSurrogate,
}
impl Transport {
    pub fn name(&self) -> &'static str {
        match self {
            Self::EnvKey(_) => "env-key",
            Self::ConnectorSurrogate => "connector-surrogate",
        }
    }
    pub fn select(c: &crate::config::Config) -> Result<Option<Self>> {
        if !c.foreverdb_connector.is_empty() {
            return Ok(Some(Self::ConnectorSurrogate));
        }
        let key = if !c.foreverdb_api_key.is_empty() {
            c.foreverdb_api_key.expose().to_owned()
        } else {
            match std::fs::read_to_string(&c.foreverdb_api_key_file) {
                Ok(k) => k.trim().into(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(_) => return Err(Failure::unreachable("ForeverDB API key file unreadable")),
            }
        };
        Ok((!key.is_empty()).then_some(Self::EnvKey(ApiKey(key))))
    }
    pub fn validate(&self, context: &Context<'_>) -> Result<()> {
        match self {
            Self::EnvKey(_) => Ok(()),
            Self::ConnectorSurrogate => shim(context, None).map(|_| ()),
        }
    }
    pub fn request(&self, path: &str, context: &Context<'_>) -> Result<Vec<u8>> {
        let url = format!("{}{path}", context.config.foreverdb_base()?);
        match self {
            Self::EnvKey(key) => {
                let mut req = Request::get(url)
                    .header("User-Agent", USER_AGENT)
                    .header("X-API-Key", &key.0);
                req.redirects = Redirects::Refuse;
                context.http.request(&req).map(|r| r.body)
            }
            Self::ConnectorSurrogate => shim(context, Some(&url)),
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum ShimResponse {
    Ok { body: Option<String> },
    Http { status: u16, body: String },
    Missing,
    Import,
    Prepare,
    Transport,
}
fn shim(context: &Context<'_>, url: Option<&str>) -> Result<Vec<u8>> {
    let output = Command::new("python3")
        .args(["-c", include_str!("foreverdb_connector.py")])
        .arg(&context.config.dynamic_credentials_dir)
        .arg(&context.config.foreverdb_connector)
        .arg(url.unwrap_or(""))
        .arg(USER_AGENT)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| Failure::unreachable("ForeverDB connector helper could not be loaded"))?;
    let reply: ShimResponse = serde_json::from_slice(&output.stdout)
        .map_err(|_| Failure::unreachable("ForeverDB connector helper could not be loaded"))?;
    match reply {
        ShimResponse::Ok { body } => {
            crate::primitives::base64_decode(body.as_deref().unwrap_or(""))
                .map_err(|_| Failure::unreachable("ForeverDB transport failed"))
        }
        ShimResponse::Http { status, body } => {
            let body = crate::primitives::base64_decode(&body)
                .ok()
                .and_then(|b| crate::http::decode_body(&b).ok())
                .unwrap_or(Document::Null);
            Err(Failure {
                message: "ForeverDB HTTP error".into(),
                code: crate::ExitCode::Unreachable,
                body: Some(Document::object([
                    ("status", i64::from(status).into()),
                    ("response", body),
                ])),
            })
        }
        ShimResponse::Missing => Err(Failure::unreachable(
            "ForeverDB connector mode requires dynamic_credentials.add_surrogate_to_request in EVERLEDGER_DYNAMIC_CREDENTIALS_DIR",
        )),
        ShimResponse::Import => Err(Failure::unreachable(
            "ForeverDB connector helper could not be loaded",
        )),
        ShimResponse::Prepare => Err(Failure::unreachable(
            "ForeverDB connector request preparation failed",
        )),
        ShimResponse::Transport => Err(Failure::unreachable("ForeverDB transport failed")),
    }
}
