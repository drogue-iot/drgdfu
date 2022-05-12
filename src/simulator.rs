use crate::firmware::FirmwareDevice;
use async_trait::async_trait;

// Device simulator where the first 4 bytes is used as version string. When swapped, new version is updated
pub struct DeviceSimulator {
    version: String,
}

impl DeviceSimulator {
    pub fn new() -> Self {
        Self {
            version: "0000".to_string(),
        }
    }
}

#[async_trait]
impl FirmwareDevice for DeviceSimulator {
    const MTU: u32 = 64;
    async fn version(&mut self) -> Result<String, anyhow::Error> {
        Ok(self.version.clone())
    }

    async fn start(&mut self, _: &str) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn status(&mut self) -> Result<Option<(u32, String)>, anyhow::Error> {
        Ok(None)
    }

    async fn write(&mut self, _: u32, _: &[u8]) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn swap(&mut self, version: &str, _: [u8; 32]) -> Result<(), anyhow::Error> {
        self.version = version.to_string();
        Ok(())
    }

    async fn synced(&mut self) -> Result<(), anyhow::Error> {
        Ok(())
    }
}
