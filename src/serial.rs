use crate::firmware::FirmwareDevice;
use anyhow::anyhow;
use anyhow::Error;
use async_trait::async_trait;
use core::future::Future;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::time::sleep;
use tokio_serial::{SerialPort, SerialPortBuilder, SerialStream};

pub struct SerialUpdater {
    port: SerialStream,
}

impl SerialUpdater {
    pub fn new(port: &PathBuf) -> Result<Self, anyhow::Error> {
        let p: String = port.to_str().unwrap().to_string();
        let builder = tokio_serial::new(p, 115200);
        Ok(Self {
            port: SerialStream::open(&builder)?,
        })
    }
}

#[repr(u8)]
pub enum SerialCommand {
    Version = 1,
    Start = 2,
    Write = 3,
    Swap = 4,
    Sync = 5,
}

#[repr(u8)]
pub enum SerialResponse {
    Ok = 1,
    Error = 2,
}

#[async_trait]
impl FirmwareDevice for SerialUpdater {
    const MTU: u32 = 128;
    async fn version(&mut self) -> Result<String, anyhow::Error> {
        log::info!("Reading version");
        self.port.write(&[SerialCommand::Version as u8]).await?;

        let mut rx = [0; u8::MAX as usize];
        self.port.read_exact(&mut rx[..1]).await?;

        let len = rx[0] as usize;

        self.port.read_exact(&mut rx[..len]).await?;
        log::info!("Read {} bytes", len);
        Ok(core::str::from_utf8(&rx[..len])?.to_string())
    }

    async fn start(&mut self) -> Result<(), anyhow::Error> {
        self.port.write(&[SerialCommand::Start as u8]).await?;

        let mut rx = [0; 1];
        self.port.read_exact(&mut rx).await?;
        if rx[0] == 1 {
            Ok(())
        } else {
            Err(anyhow!("Error triggering DFU process"))
        }
    }
    async fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), anyhow::Error> {
        log::info!("Writing DFU offset {} len {}", offset, data.len());
        self.port.write(&[SerialCommand::Write as u8]).await?;
        self.port.write(&offset.to_le_bytes()).await?;
        self.port.write(&data.len().to_le_bytes()).await?;
        self.port.write(&data).await?;

        let mut rx = [0; 1];
        self.port.read_exact(&mut rx).await?;
        if rx[0] == 1 {
            log::info!("Data written!");
            sleep(Duration::from_secs(2)).await;
            Ok(())
        } else {
            log::warn!("Error writing data");
            Err(anyhow!("Error writing DFU packet"))
        }
    }
    async fn swap(&mut self) -> Result<(), anyhow::Error> {
        todo!()
    }

    async fn synced(&mut self) -> Result<(), anyhow::Error> {
        todo!()
    }
}
