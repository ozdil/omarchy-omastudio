//! PTP over USB with nusb (pure Rust, no libusb). Needs read/write access to the
//! camera's /dev/bus/usb node; the packaged udev rule grants it to the seat user.

use super::{Error, Result, Transport};
use nusb::transfer::{Buffer, Bulk, In, Interrupt, Out};
use nusb::{Endpoint, Interface, MaybeFuture};
use std::time::Duration;

/// USB interface class of still-image (PTP) devices.
const STILL_IMAGE_CLASS: u8 = 6;
/// Size of each bulk IN request; a multiple of every max packet size.
const CHUNK: usize = 1024 * 1024;

/// A camera found on the bus.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CameraInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: String,
    pub product: String,
    pub serial: String,
    pub bus: String,
    pub address: u8,
}

/// Cameras exposing a PTP interface, optionally only from one vendor.
pub fn list_cameras(vendor: Option<u16>) -> Result<Vec<CameraInfo>> {
    let devices = nusb::list_devices().wait().map_err(|e| Error::Usb(e.to_string()))?;
    Ok(devices
        .filter(|d| vendor.is_none_or(|v| d.vendor_id() == v))
        .filter(|d| d.class() == STILL_IMAGE_CLASS || d.interfaces().any(|i| i.class() == STILL_IMAGE_CLASS))
        .map(|d| CameraInfo {
            vendor_id: d.vendor_id(),
            product_id: d.product_id(),
            manufacturer: d.manufacturer_string().unwrap_or_default().to_string(),
            product: d.product_string().unwrap_or_default().to_string(),
            serial: d.serial_number().unwrap_or_default().to_string(),
            bus: d.bus_id().to_string(),
            address: d.device_address(),
        })
        .collect())
}

pub struct UsbTransport {
    _interface: Interface,
    bulk_out: Endpoint<Bulk, Out>,
    bulk_in: Endpoint<Bulk, In>,
    events: Option<Endpoint<Interrupt, In>>,
    /// Bytes received past the end of the last container.
    pending: Vec<u8>,
}

impl UsbTransport {
    /// Opens the first PTP camera matching `vendor` (and `product`, when given).
    pub fn open(vendor: Option<u16>, product: Option<u16>) -> Result<Self> {
        let info = nusb::list_devices()
            .wait()
            .map_err(|e| Error::Usb(e.to_string()))?
            .filter(|d| vendor.is_none_or(|v| d.vendor_id() == v) && product.is_none_or(|p| d.product_id() == p))
            .find(|d| d.interfaces().any(|i| i.class() == STILL_IMAGE_CLASS))
            .ok_or_else(|| Error::Usb("No camera in PTP/USB mode found. Set the camera's USB mode to \"USB RAW CONV./BACKUP RESTORE\" or tethering and connect it.".into()))?;
        let device = info.open().wait().map_err(|e| permission_hint(e.to_string()))?;
        let config = device.active_configuration().map_err(|e| Error::Usb(e.to_string()))?;
        let alt = config
            .interface_alt_settings()
            .find(|a| a.class() == STILL_IMAGE_CLASS)
            .ok_or_else(|| Error::Usb("Camera has no still-image interface".into()))?;
        let number = alt.interface_number();
        let (mut ep_out, mut ep_in, mut ep_event) = (None, None, None);
        for ep in alt.endpoints() {
            use nusb::descriptors::TransferType;
            use nusb::transfer::Direction;
            match (ep.transfer_type(), ep.direction()) {
                (TransferType::Bulk, Direction::Out) => ep_out = ep_out.or(Some(ep.address())),
                (TransferType::Bulk, Direction::In) => ep_in = ep_in.or(Some(ep.address())),
                (TransferType::Interrupt, Direction::In) => ep_event = ep_event.or(Some(ep.address())),
                _ => {}
            }
        }
        let (Some(out_addr), Some(in_addr)) = (ep_out, ep_in) else {
            return Err(Error::Usb("Camera has no bulk endpoints".into()));
        };
        let interface = device.claim_interface(number).wait().map_err(|e| {
            Error::Usb(format!(
                "Camera is in use by another program ({}). Close gphoto2/gvfs or the file manager's camera view and try again.",
                e
            ))
        })?;
        let bulk_out = interface.endpoint::<Bulk, Out>(out_addr).map_err(|e| Error::Usb(e.to_string()))?;
        let bulk_in = interface.endpoint::<Bulk, In>(in_addr).map_err(|e| Error::Usb(e.to_string()))?;
        let events = ep_event.and_then(|a| interface.endpoint::<Interrupt, In>(a).ok());
        Ok(Self { _interface: interface, bulk_out, bulk_in, events, pending: Vec::new() })
    }

    fn read_chunk(&mut self, timeout: Duration) -> Result<Vec<u8>> {
        let c = self.bulk_in.transfer_blocking(Buffer::new(CHUNK), timeout);
        match c.status {
            Ok(()) => Ok(c.buffer[..c.actual_len.min(c.buffer.len())].to_vec()),
            Err(nusb::transfer::TransferError::Cancelled) => Err(Error::Timeout),
            Err(e) => Err(Error::Usb(e.to_string())),
        }
    }
}

fn permission_hint(e: String) -> Error {
    Error::Usb(format!(
        "Cannot open the camera ({}). Install the OmaStudio udev rule (70-omastudio-cameras.rules) and replug the camera.",
        e
    ))
}

impl Transport for UsbTransport {
    fn send(&mut self, container: &[u8], timeout: Duration) -> Result<()> {
        // nusb splits large buffers into packets itself; a short last packet ends the
        // transfer, an exact multiple needs a zero-length packet.
        let mps = self.bulk_out.max_packet_size();
        let mut parts: Vec<&[u8]> = container.chunks(CHUNK * 16).collect();
        if container.len().is_multiple_of(mps) {
            parts.push(&[]);
        }
        for part in parts {
            let mut buf = Buffer::new(part.len());
            buf.extend_from_slice(part);
            let c = self.bulk_out.transfer_blocking(buf, timeout);
            match c.status {
                Ok(()) => {}
                Err(nusb::transfer::TransferError::Cancelled) => return Err(Error::Timeout),
                Err(e) => return Err(Error::Usb(e.to_string())),
            }
        }
        Ok(())
    }

    fn receive(&mut self, timeout: Duration) -> Result<Vec<u8>> {
        let mut data = std::mem::take(&mut self.pending);
        while data.len() < 4 {
            // Zero-length packets that end the previous container are skipped here
            data.extend(self.read_chunk(timeout)?);
        }
        let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if !(12..=super::MAX_DATA).contains(&len) {
            return Err(Error::Protocol(format!("container length {}", len)));
        }
        data.reserve(len.saturating_sub(data.len()));
        while data.len() < len {
            data.extend(self.read_chunk(timeout)?);
        }
        self.pending = data.split_off(len);
        Ok(data)
    }

    fn event(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let Some(ep) = self.events.as_mut() else { return Ok(None) };
        let mps = ep.max_packet_size().max(64);
        let c = ep.transfer_blocking(Buffer::new(mps), timeout);
        match c.status {
            Ok(()) => Ok(Some(c.buffer[..c.actual_len.min(c.buffer.len())].to_vec())),
            Err(nusb::transfer::TransferError::Cancelled) => Ok(None),
            Err(e) => Err(Error::Usb(e.to_string())),
        }
    }
}
