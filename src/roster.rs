use crate::{
    Result,
    config::Config,
    document::Document,
    http::{Http, Payload, Request, decode_body},
    primitives::{quote, urlencode},
    time::roster_seconds,
};

pub fn character_path(name: &str) -> String {
    format!("/characters/{}", quote(name))
}
pub fn roster<T: serde::de::DeserializeOwned>(
    http: &dyn Http,
    config: &Config,
    path: &str,
    params: &[(String, Document)],
) -> Result<Payload<T>> {
    let token = config.roster_token()?;
    let mut params = params
        .iter()
        .filter(|(_, v)| !matches!(v, Document::Null))
        .cloned()
        .collect::<Vec<_>>();
    if path == "/events" {
        for (k, v) in &mut params {
            if k == "since" || k == "until" {
                *v = Document::from(roster_seconds(v)?.0);
            }
        }
    }
    let query = urlencode(&params)?;
    let url = format!(
        "{}{path}{}{}",
        config.roster_url,
        if query.is_empty() { "" } else { "?" },
        query
    );
    let response =
        http.request(&Request::get(url).header("Authorization", format!("Bearer {token}")))?;
    Payload::decode(decode_body(&response.body)?, "roster")
}
