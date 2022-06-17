use btleplug::api::{BDAddr, Central, Characteristic, Peripheral as _, WriteType};
use btleplug::platform::{Adapter, Peripheral};
use core::future::Future;
use embedded_update::*;
use tokio::time::{sleep, Duration};

pub struct GattBoard {
    adapter: Adapter,
    address: BDAddr,
    board: Option<Peripheral>,
    updated: bool,
    mtu: Option<u8>,
}

const FIRMWARE_SERVICE_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001000b0cd11ec871fd45ddf138840);

const VERSION_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001001b0cd11ec871fd45ddf138840);
const MTU_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001002b0cd11ec871fd45ddf138840);
const CONTROL_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001003b0cd11ec871fd45ddf138840);
const NEXT_VERSION_CHAR_UUID: uuid::Uuid =
    uuid::Uuid::from_u128(0x00001004b0cd11ec871fd45ddf138840);
const OFFSET_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001005b0cd11ec871fd45ddf138840);
const FIRMWARE_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001006b0cd11ec871fd45ddf138840);

impl GattBoard {
    pub fn new(device: &str, adapter: Adapter) -> Self {
        Self {
            address: BDAddr::from_str_delim(device).unwrap(),
            adapter,
            board: None,
            updated: false,
            mtu: None,
        }
    }

    async fn read_firmware_offset(&mut self) -> anyhow::Result<u32> {
        let data = self
            .read_char(FIRMWARE_SERVICE_UUID, OFFSET_CHAR_UUID)
            .await?;
        Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    }

    async fn read_firmware_version(&mut self) -> anyhow::Result<Vec<u8>> {
        let data = self
            .read_char(FIRMWARE_SERVICE_UUID, VERSION_CHAR_UUID)
            .await?;
        Ok(data)
    }

    async fn read_mtu(&mut self) -> anyhow::Result<u8> {
        let data = self.read_char(FIRMWARE_SERVICE_UUID, MTU_CHAR_UUID).await?;
        Ok(data[0])
    }

    async fn read_next_firmware_version(&mut self) -> anyhow::Result<Vec<u8>> {
        let data = self
            .read_char(FIRMWARE_SERVICE_UUID, NEXT_VERSION_CHAR_UUID)
            .await?;
        Ok(data)
    }

    async fn mark_booted(&mut self) -> anyhow::Result<()> {
        // Trigger DFU process
        self.write_char(FIRMWARE_SERVICE_UUID, CONTROL_CHAR_UUID, &[3])
            .await
    }

    async fn start_firmware_update(&mut self, version: &[u8]) -> Result<(), anyhow::Error> {
        // Write the version we're updating
        self.write_char(FIRMWARE_SERVICE_UUID, NEXT_VERSION_CHAR_UUID, version)
            .await?;

        // Trigger DFU process
        self.write_char(FIRMWARE_SERVICE_UUID, CONTROL_CHAR_UUID, &[1])
            .await?;

        // Wait until firmware offset is reset
        while self.read_firmware_offset().await? != 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        Ok(())
    }

    async fn write_firmware(
        &mut self,
        mut offset: u32,
        firmware: &[u8],
    ) -> Result<(), anyhow::Error> {
        // Retrieve desired MTU size
        if self.mtu.is_none() {
            let mtu = self.read_mtu().await?;
            self.mtu.replace(mtu);
        }

        let mtu = self.mtu.unwrap() as usize;
        let mut buf = [0; u8::MAX as usize];
        for chunk in firmware.chunks(mtu) {
            buf[0..chunk.len()].copy_from_slice(chunk);
            if chunk.len() < mtu {
                buf[chunk.len()..mtu].fill(0);
            }
            self.write_char(FIRMWARE_SERVICE_UUID, FIRMWARE_CHAR_UUID, &buf[0..mtu])
                .await?;
            log::debug!("Write {} bytes at offset {}", mtu, offset);
            offset += mtu as u32;
            if offset % 4096 == 0 {
                println!("{} bytes written", offset)
            }

            // Wait until firmware offset is incremented
            while self.read_firmware_offset().await? != offset {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
        Ok(())
    }

    async fn swap_firmware(&mut self) -> Result<(), anyhow::Error> {
        // Write signal that DFU process is done and should be applied
        log::info!("DFU process done, setting reset");

        self.write_char(FIRMWARE_SERVICE_UUID, CONTROL_CHAR_UUID, &[2])
            .await?;

        Ok(())
    }

    async fn read_char(&mut self, service: uuid::Uuid, c: uuid::Uuid) -> anyhow::Result<Vec<u8>> {
        let (device, c) = self.find_char(service, c).await?;
        if let Some(c) = c {
            let value = device.read(&c).await?;
            Ok(value)
        } else {
            Err(anyhow::anyhow!("unable to locate characteristic"))
        }
    }

    async fn write_char(
        &mut self,
        service: uuid::Uuid,
        c: uuid::Uuid,
        value: &[u8],
    ) -> anyhow::Result<()> {
        let (device, c) = self.find_char(service, c).await?;
        if let Some(c) = c {
            device.write(&c, value, WriteType::WithResponse).await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("unable to locate characteristic"))
        }
    }

    async fn find_char(
        &mut self,
        service: uuid::Uuid,
        characteristic: uuid::Uuid,
    ) -> anyhow::Result<(&mut Peripheral, Option<Characteristic>)> {
        let device = self.connect().await?;
        for s in device.services() {
            if s.uuid == service {
                for c in s.characteristics {
                    if c.uuid == characteristic {
                        return Ok((device, Some(c)));
                    }
                }
            }
        }
        Ok((device, None))
    }

    async fn connect(&mut self) -> anyhow::Result<&mut Peripheral> {
        if self.board.is_none() {
            loop {
                for device in self.adapter.peripherals().await? {
                    if let Some(p) = device.properties().await? {
                        if p.address == self.address {
                            // Make sure we get a fresh start
                            let _ = device.disconnect().await;
                            sleep(Duration::from_secs(2)).await;
                            match device.is_connected().await {
                                Ok(false) => {
                                    log::info!("Connecting...");
                                    loop {
                                        match device.connect().await {
                                            Ok(()) => break,
                                            Err(err) => {
                                                log::error!("Connect error: {}", &err);
                                            }
                                        }
                                    }
                                    log::info!("Connected!");
                                    device.discover_services().await?;
                                    self.board.replace(device);
                                    return Ok(self.board.as_mut().unwrap());
                                }
                                Ok(true) => {
                                    log::info!("Connected!");
                                    self.board.replace(device);
                                    return Ok(self.board.as_mut().unwrap());
                                }
                                Err(e) => {
                                    log::info!("Error checking connection, retrying: {:?}", e);
                                }
                            }
                        }
                    }
                }
                sleep(Duration::from_secs(2)).await;
            }
        }
        Ok(self.board.as_mut().unwrap())
    }
}

impl FirmwareDevice for GattBoard {
    const MTU: usize = 4096;
    type Version = Vec<u8>;
    type Error = anyhow::Error;

    type StatusFuture<'m> = impl Future<Output = Result<FirmwareStatus<Self::Version>, Self::Error>> + 'm
    where
        Self: 'm;

    fn status(&mut self) -> Self::StatusFuture<'_> {
        async move {
            let version = self.read_firmware_version().await?;
            let next = self.read_next_firmware_version().await?;
            let offset = self.read_firmware_offset().await?;
            log::trace!(
                "Current: {:?}, next: {:?}, next offset: {:?}",
                version,
                next,
                offset
            );
            Ok(FirmwareStatus {
                current_version: version,
                next_version: Some(next),
                next_offset: offset,
            })
        }
    }

    type StartFuture<'m> = impl Future<Output = Result<(), Self::Error>> + 'm

    where
        Self: 'm;

    fn start<'m>(&'m mut self, version: &'m [u8]) -> Self::StartFuture<'m> {
        async move { Ok(self.start_firmware_update(version).await?) }
    }

    type WriteFuture<'m> = impl Future<Output = Result<(), Self::Error>> + 'm

    where
        Self: 'm;

    fn write<'m>(&'m mut self, offset: u32, data: &'m [u8]) -> Self::WriteFuture<'m> {
        async move { Ok(self.write_firmware(offset, data).await?) }
    }

    type UpdateFuture<'m> = impl Future<Output = Result<(), Self::Error>> + 'm

    where
        Self: 'm;

    fn update<'m>(&'m mut self, _: &'m [u8], _: &'m [u8]) -> Self::UpdateFuture<'m> {
        async move {
            log::debug!("Swapping firmware");
            let r = Ok(self.swap_firmware().await?);
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            if let Some(board) = self.board.take() {
                let _ = board.disconnect().await;
            }
            self.updated = true;
            r
        }
    }

    type SyncedFuture<'m> = impl Future<Output = Result<(), Self::Error>> + 'm

    where
        Self: 'm;

    fn synced(&mut self) -> Self::SyncedFuture<'_> {
        async move {
            if self.updated {
                log::debug!("Mark as booted");
                self.updated = false;
                Ok(self.mark_booted().await?)
            } else {
                log::debug!("Not updated?!");
                Ok(())
            }
        }
    }
}
