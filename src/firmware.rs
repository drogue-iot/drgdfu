use anyhow::anyhow;
use bytes::Bytes;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug)]
pub struct FirmwareFileMeta {
    pub version: String,
    pub size: usize,
    pub file: PathBuf,
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FirmwareSource {
    File {
        file: PathBuf,
    },
    Cloud {
        url: String,
        application: String,
        token: String,
    },
}

pub struct Report {
    version: String,
    mtu: Option<u32>,
    last_version: Option<String>,
    last_offset: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Command {
    op: Operation,
    version: String,
    poll: Option<u32>,
    offset: Option<u32>,
    data: Option<Vec<u8>>,
    checksum: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Operation {
    Sync,
    Write,
    Swap,
}

impl FirmwareSource {
    pub async fn report(&mut self, report: &Report) -> Result<Command, FirmwareError> {
        todo!()
    }
}

#[derive(Debug)]
pub enum FirmwareError {
    Io(std::io::Error),
    Parse(serde_json::Error),
}

impl FirmwareFileMeta {
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

/*


impl FirmwareMetadata {
    pub fn from_file(path: &Path) -> Result<Self, FirmwareError> {
        let data = std::fs::read_to_string(path)?;
        let metadata = serde_json::from_str(&data)?;
        Ok(metadata)
    }

    pub fn from_http(url: String, size: usize, version: String) -> Self {
        Self {
            version,
            size,
            data: FirmwareData::Http(url),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum FirmwareData {
    #[serde(rename = "file")]
    File(PathBuf),
    #[serde(rename = "http")]
    Http(String),
}

pub struct Deployment {
    pub id: String,
    pub metadata: FirmwareMetadata,
}

pub struct FirmwareClient {
    url: String,
}

impl FirmwareClient {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    pub async fn fetch_firmware(&self, url: &str) -> Result<Bytes, anyhow::Error> {
        let client = reqwest::Client::new();
        let res = client.get(url).send().await?.bytes().await?;
        Ok(res)
    }

    pub async fn wait_update(&self, current_version: &str) -> Result<Deployment, anyhow::Error> {
        let client = reqwest::Client::new();
        loop {
            let url = format!("{}/v1/poll/burrboard", self.url);
            let res = client.get(&url).send().await;

            let poll: Duration = match res {
                Ok(res) => {
                    let j: serde_json::Value = res.json().await.unwrap();
                    println!("RESULT:{:?}", j);

                    if let Some(current) = j.get("current") {
                        if let Some(version) = current["version"].as_str() {
                            if version != current_version {
                                let size = current["size"]
                                    .as_str()
                                    .ok_or(anyhow!("error reading firmware size"))?
                                    .parse::<usize>()?;
                                let path = format! {"{}/v1/fetch/burrboard/{}", self.url, version};
                                return Ok(Deployment {
                                    id: version.to_string(),
                                    metadata: FirmwareMetadata::from_http(
                                        path,
                                        size as usize,
                                        version.to_string(),
                                    ),
                                });
                            }
                        }
                    }

                    if let Some(interval) = j["interval"].as_i64() {
                        Duration::from_secs(interval as u64)
                    } else {
                        Duration::from_secs(5)
                    }
                }
                Err(e) => {
                    println!("ERROR FIRMWARE: {:?}", e);
                    Duration::from_secs(5)
                }
            };
            println!("Polling firmware server in {} seconds", poll.as_secs());
            tokio::time::sleep(poll).await;
        }
    }
}
*/
