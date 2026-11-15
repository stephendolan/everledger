use crate::{Failure, Result};
use std::{collections::HashMap, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub data_dir: PathBuf,
    pub roster_url: String,
    pub ahledger_url: String,
    pub market: String,
    pub vendor_url: String,
    pub roster_token: String,
    pub roster_token_file: Option<PathBuf>,
    pub foreverdb_api_key: String,
    pub foreverdb_api_key_file: PathBuf,
    pub foreverdb_connector: String,
    pub dynamic_credentials_dir: PathBuf,
    #[cfg(feature = "parity-seams")]
    pub foreverdb_url: Option<String>,
}
impl Config {
    pub fn from_env() -> Self {
        Self::from_values(
            &std::env::vars_os()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
                .collect(),
        )
    }
    pub fn from_values(env: &HashMap<String, String>) -> Self {
        let get = |k: &str, d: &str| env.get(k).cloned().unwrap_or_else(|| d.into());
        let data_dir = PathBuf::from(get(
            "EVERLEDGER_DATA_DIR",
            &format!("{}/.everledger", get("HOME", ".")),
        ));
        Self {
            roster_url: get("EVERLEDGER_ROSTER_URL", "http://localhost:8765")
                .trim_end_matches('/')
                .into(),
            ahledger_url: get("EVERLEDGER_AHLEDGER_URL", "https://api.ahledger.com")
                .trim_end_matches('/')
                .into(),
            market: get("EVERLEDGER_MARKET", "forever.pvp.alliance.us"),
            vendor_url: get("EVERLEDGER_VENDOR_URL", "https://wowforever.ninja")
                .trim_end_matches('/')
                .into(),
            roster_token: get("EVERLEDGER_ROSTER_TOKEN", "").trim().into(),
            roster_token_file: env
                .get("EVERLEDGER_ROSTER_TOKEN_FILE")
                .filter(|s| !s.is_empty())
                .map(Into::into),
            foreverdb_api_key: get("EVERLEDGER_FOREVERDB_API_KEY", "").trim().into(),
            foreverdb_api_key_file: env
                .get("EVERLEDGER_FOREVERDB_API_KEY_FILE")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| data_dir.join("foreverdb-api-key")),
            foreverdb_connector: get("EVERLEDGER_FOREVERDB_VIA_CONNECTOR", "").trim().into(),
            dynamic_credentials_dir: PathBuf::from(get(
                "EVERLEDGER_DYNAMIC_CREDENTIALS_DIR",
                "/opt/muse/skills/custom-connector-setup/bin",
            )),
            data_dir,
            #[cfg(feature = "parity-seams")]
            foreverdb_url: env.get("EVERLEDGER_SEAM_FOREVERDB_URL").cloned(),
        }
    }
    pub fn roster_token(&self) -> Result<String> {
        let mut token = self.roster_token.clone();
        if token.is_empty()
            && let Some(path) = &self.roster_token_file
        {
            token = std::fs::read_to_string(path)?.trim().into();
        }
        if token.is_empty() {
            Err(Failure::unreachable(
                "configure EVERLEDGER_ROSTER_TOKEN or EVERLEDGER_ROSTER_TOKEN_FILE",
            ))
        } else {
            Ok(token)
        }
    }
    pub fn foreverdb_base(&self) -> Result<&str> {
        #[cfg(feature = "parity-seams")]
        {
            self.foreverdb_url.as_deref().ok_or_else(|| {
                Failure::unreachable("ForeverDB disabled without EVERLEDGER_SEAM_FOREVERDB_URL")
            })
        }
        #[cfg(not(feature = "parity-seams"))]
        {
            Ok("https://foreverdb.net")
        }
    }
}
pub const FOREVERDB_DAILY_REQUESTS: u32 = 36;
