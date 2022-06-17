use anyhow::anyhow;
use core::future::Future;
use embedded_update::*;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug)]
pub struct FirmwareFileMeta {
    pub version: String,
    pub size: usize,
    pub checksum: String,
}

pub struct DrogueFirmwareService {
    pub url: String,
    pub user: String,
    pub password: String,
    pub timeout: std::time::Duration,
    pub client: reqwest::Client,
    pub last_response: Vec<u8>,
}

impl DrogueFirmwareService {
    pub fn new(url: &str, user: &str, password: &str, timeout: std::time::Duration) -> Self {
        Self {
            url: url.to_string(),
            user: user.to_string(),
            password: password.to_string(),
            timeout,
            client: reqwest::Client::new(),
            last_response: Vec::new(),
        }
    }
}

impl embedded_update::UpdateService for DrogueFirmwareService {
    type Error = anyhow::Error;

    type RequestFuture<'m> = impl Future<Output = Result<Command<'m>, Self::Error>> + 'm
    where
        Self: 'm;

    fn request<'m>(&'m mut self, status: &'m Status<'m>) -> Self::RequestFuture<'m> {
        async move {
            let payload = serde_cbor::to_vec(status)?;
            let mut query: Vec<(String, String)> = Vec::new();
            query.push(("ct".to_string(), format!("{}", self.timeout.as_secs())));
            /* TODO: act on behalf of device
            if let Some(name) = name {
                query.push(("as".to_string(), name.to_string()));
            }
            */

            let url = format!("{}/v1/dfu", self.url);
            let result = self
                .client
                .post(url)
                .basic_auth(&self.user, Some(&self.password))
                .query(&query[..])
                .body(payload)
                .send()
                .await;

            match result {
                Ok(r) if !r.status().is_success() => Err(anyhow!(
                    "Error reporting status to cloud: {}: {}",
                    r.status(),
                    r.text().await.unwrap_or_default()
                )),
                Ok(r) => {
                    if let Ok(payload) = r.bytes().await {
                        log::trace!("Received command: {:?}", payload);
                        {
                            self.last_response.clear();
                            self.last_response.extend(payload);
                        }
                        if let Ok(cmd) = serde_cbor::de::from_mut_slice::<Command<'m>>(
                            &mut self.last_response[..],
                        ) {
                            Ok(cmd)
                        } else {
                            Err(anyhow!("Error parsing command"))
                        }
                    } else {
                        Err(anyhow!("Error retrieving payload"))
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

#[derive(Debug)]
pub enum FirmwareError {
    Io(std::io::Error),
    Parse(serde_json::Error),
}

impl FirmwareFileMeta {
    pub fn new(version: &str, path: &PathBuf) -> Result<Self, FirmwareError> {
        let f = File::open(path)?;
        let metadata = f.metadata()?;
        let len = metadata.len();
        Ok(Self {
            version: version.to_string(),
            size: len as usize,
            checksum: String::new(),
        })
    }
    pub fn from_file(path: &PathBuf) -> Result<Self, FirmwareError> {
        let data = std::fs::read_to_string(path)?;
        let metadata = serde_json::from_str(&data)?;
        Ok(metadata)
    }
}

impl core::fmt::Display for FirmwareError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), core::fmt::Error> {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Parse(e) => e.fmt(f),
        }
    }
}

impl From<std::io::Error> for FirmwareError {
    fn from(error: std::io::Error) -> Self {
        FirmwareError::Io(error)
    }
}

impl From<serde_json::Error> for FirmwareError {
    fn from(error: serde_json::Error) -> Self {
        FirmwareError::Parse(error)
    }
}

impl serde::ser::StdError for FirmwareError {}
