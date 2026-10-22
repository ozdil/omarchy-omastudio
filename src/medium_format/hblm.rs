//! Hasselblad tethering ("hblm" messaging). EXPERIMENTAL, not yet tried on a camera.
//!
//! X and V system cameras (X1D, X2D, 907X/CFV) do not speak PTP. Phocus talks to them
//! over a vendor-specific USB interface (class 0xFF) with small signal messages
//! routed between the camera's processors ("nodes"). This module implements the
//! message framing as documented in docs/medium-format.md: worked out clean-room from
//! the behaviour of Phocus 4.2 (macOS), no Hasselblad code is used. How the frames are
//! packed into bulk transfers on the wire still has to be confirmed with a usbmon
//! capture of a real camera; until then the commands are offered behind
//! `--experimental`.

use nusb::transfer::{Buffer, Bulk, In, Out};
use nusb::{Endpoint, Interface, MaybeFuture};
use serde::Serialize;
use std::time::{Duration, Instant};

/// Victor Hasselblad AB.
pub const VENDOR_HASSELBLAD: u16 = 0x2756;
/// DJI, which builds the X2D/907X electronics and enumerates some of them.
pub const VENDOR_DJI: u16 = 0x2ca3;
const VENDOR_CLASS: u8 = 0xff;

/// Signal ids: the index in the camera's signal table. Only the ones used here.
pub mod signal {
    pub const SUC_FIRMWARE_VERSION_REQ: u16 = 0x000b;
    pub const SUC_FIRMWARE_VERSION_RESP: u16 = 0x000c;
    pub const FARM_FIRMWARE_VERSION_REQ: u16 = 0x000d;
    pub const FARM_FIRMWARE_VERSION_RESP: u16 = 0x000e;
    pub const EXPOSURE_START_EVENT: u16 = 0x002c;
    pub const EXPOSURE_DONE_EVENT: u16 = 0x002f;
    pub const DO_EXPOSURE_REQ: u16 = 0x00c4;
    pub const DO_EXPOSURE_RESP: u16 = 0x00c5;
    pub const EXPOSURES_LEFT_REQ: u16 = 0x0201;
    pub const EXPOSURES_LEFT_RESP: u16 = 0x0202;
    pub const GET_ISO_REQ: u16 = 0x02ed;
    pub const GET_ISO_RESP: u16 = 0x02ee;
    pub const SET_ISO_REQ: u16 = 0x02ef;
    pub const SET_ISO_RESP: u16 = 0x02f0;
}

/// The processors inside the camera that exchange messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[repr(u8)]
pub enum Node {
    None = 0,
    /// Main application processor.
    Farm = 1,
    PowerControl = 2,
    /// System/shutter controller.
    Suc = 3,
    /// The host side of the connection (USB or Wi-Fi).
    NetHost = 4,
    /// Image processor.
    Imx = 5,
}

/// Framing generation. Cameras with product id 7..=30 use the newer one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Protocol {
    V1,
    V2,
}

impl Protocol {
    pub fn for_device(vendor: u16, product: u16) -> Self {
        if vendor == VENDOR_HASSELBLAD && (7..=30).contains(&product) { Self::V2 } else { Self::V1 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub signal: u16,
    pub src: u8,
    pub dst: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub enum Error {
    Usb(String),
    Timeout,
    Frame(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usb(e) => write!(f, "USB error: {}", e),
            Self::Timeout => write!(f, "Camera did not answer in time"),
            Self::Frame(e) => write!(f, "Malformed Hasselblad message: {}", e),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Largest message accepted from the camera (previews are sent in many messages).
const MAX_MESSAGE: usize = 64 * 1024 * 1024;
/// v1: payload bytes in the first fragment and in each following one.
const V1_FIRST: usize = 246;
const V1_NEXT: usize = 250;

/// v2 frame: [signal u16 LE][src][dst][len u8][payload]; len 0 means a big-endian u16
/// length follows at offset 5 and the payload starts at 7.
pub fn encode_v2(m: &Message) -> Vec<u8> {
    let mut out = Vec::with_capacity(m.payload.len() + 7);
    out.extend_from_slice(&m.signal.to_le_bytes());
    out.extend_from_slice(&[m.src, m.dst]);
    if !m.payload.is_empty() && m.payload.len() < 256 {
        out.push(m.payload.len() as u8);
    } else {
        out.push(0);
        out.extend_from_slice(&(m.payload.len().min(u16::MAX as usize) as u16).to_be_bytes());
    }
    out.extend_from_slice(&m.payload);
    out
}

/// Decodes one v2 frame from the front of `buf`; returns it and the bytes it used.
pub fn decode_v2(buf: &[u8]) -> Result<Option<(Message, usize)>> {
    if buf.len() < 5 {
        return Ok(None);
    }
    let (len, start) = match buf[4] {
        0 if buf.len() < 7 => return Ok(None),
        0 => (u16::from_be_bytes([buf[5], buf[6]]) as usize, 7),
        n => (n as usize, 5),
    };
    if buf.len() < start + len {
        return Ok(None);
    }
    let m = Message { signal: u16::from_le_bytes([buf[0], buf[1]]), src: buf[2], dst: buf[3], payload: buf[start..start + len].to_vec() };
    Ok(Some((m, start + len)))
}

/// v1 splits a message into fragments: the first carries
/// [signal][src][dst][len = size-5][index u32 = 0][msg id][total u32][payload..246],
/// the rest [signal][src][dst][len][index u32][msg id][payload..250].
pub fn encode_v1(m: &Message, msg_id: u8) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let total = m.payload.len() as u32;
    let mut rest = &m.payload[..];
    let mut index = 0u32;
    loop {
        let room = if index == 0 { V1_FIRST } else { V1_NEXT };
        let (part, tail) = rest.split_at(rest.len().min(room));
        let mut f = Vec::with_capacity(part.len() + 14);
        f.extend_from_slice(&m.signal.to_le_bytes());
        f.extend_from_slice(&[m.src, m.dst, 0]);
        f.extend_from_slice(&index.to_le_bytes());
        f.push(msg_id);
        if index == 0 {
            f.extend_from_slice(&total.to_le_bytes());
        }
        f.extend_from_slice(part);
        f[4] = (f.len() - 5) as u8;
        out.push(f);
        rest = tail;
        index += 1;
        if rest.is_empty() {
            break;
        }
    }
    out
}

/// Joins v1 fragments into whole messages.
#[derive(Default)]
pub struct Reassembler {
    current: Option<(Message, u8, u32, usize)>,
}

impl Reassembler {
    /// Feeds one fragment; returns the message once its last fragment arrived.
    pub fn push(&mut self, f: &[u8]) -> Result<Option<Message>> {
        if f.len() < 10 || f[4] as usize + 5 != f.len() {
            return Err(Error::Frame(format!("fragment of {} bytes", f.len())));
        }
        let index = u32::from_le_bytes([f[5], f[6], f[7], f[8]]);
        let id = f[9];
        if index == 0 {
            if f.len() < 14 {
                return Err(Error::Frame("short first fragment".into()));
            }
            let total = u32::from_le_bytes([f[10], f[11], f[12], f[13]]) as usize;
            if total > MAX_MESSAGE {
                return Err(Error::Frame(format!("message of {} bytes", total)));
            }
            let mut payload = Vec::with_capacity(total);
            payload.extend_from_slice(&f[14..]);
            let m = Message { signal: u16::from_le_bytes([f[0], f[1]]), src: f[2], dst: f[3], payload };
            self.current = Some((m, id, 1, total));
        } else {
            match self.current.as_mut() {
                Some((m, cid, next, _)) if *cid == id && *next == index && m.signal == u16::from_le_bytes([f[0], f[1]]) => {
                    m.payload.extend_from_slice(&f[10..]);
                    *next += 1;
                }
                _ => {
                    self.current = None;
                    return Err(Error::Frame(format!("fragment {} out of order", index)));
                }
            }
        }
        match &self.current {
            Some((m, _, _, total)) if m.payload.len() >= *total => {
                let (mut m, _, _, total) = self.current.take().unwrap();
                m.payload.truncate(total);
                Ok(Some(m))
            }
            _ => Ok(None),
        }
    }
}

/// Which processor answers a request, by the signal's owner.
pub fn route(signal: u16) -> Node {
    match signal {
        signal::SUC_FIRMWARE_VERSION_REQ => Node::Suc,
        _ => Node::Farm,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub product: String,
    pub serial: String,
    pub protocol: Protocol,
    /// Whether the camera shows the vendor interface Phocus uses.
    pub vendor_interface: bool,
}

/// Hasselblad cameras on the bus.
pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    let devices = nusb::list_devices().wait().map_err(|e| Error::Usb(e.to_string()))?;
    Ok(devices
        .filter(|d| d.vendor_id() == VENDOR_HASSELBLAD || d.vendor_id() == VENDOR_DJI)
        .map(|d| DeviceInfo {
            vendor_id: d.vendor_id(),
            product_id: d.product_id(),
            product: d.product_string().unwrap_or_default().to_string(),
            serial: d.serial_number().unwrap_or_default().to_string(),
            protocol: Protocol::for_device(d.vendor_id(), d.product_id()),
            vendor_interface: d.interfaces().any(|i| i.class() == VENDOR_CLASS),
        })
        .collect())
}

/// A tethered camera.
pub struct Camera {
    _interface: Interface,
    bulk_out: Endpoint<Bulk, Out>,
    bulk_in: Endpoint<Bulk, In>,
    protocol: Protocol,
    msg_id: u8,
    pending: Vec<u8>,
    reassembler: Reassembler,
}

impl Camera {
    pub fn open() -> Result<Self> {
        let info = nusb::list_devices()
            .wait()
            .map_err(|e| Error::Usb(e.to_string()))?
            .filter(|d| d.vendor_id() == VENDOR_HASSELBLAD || d.vendor_id() == VENDOR_DJI)
            .find(|d| d.interfaces().any(|i| i.class() == VENDOR_CLASS))
            .ok_or_else(|| Error::Usb("No Hasselblad camera found. Connect it with USB-C and switch it on.".into()))?;
        let protocol = Protocol::for_device(info.vendor_id(), info.product_id());
        let device = info.open().wait().map_err(|e| {
            Error::Usb(format!("Cannot open the camera ({}). Install the OmaStudio udev rule (70-omastudio-cameras.rules) and replug the camera.", e))
        })?;
        let config = device.active_configuration().map_err(|e| Error::Usb(e.to_string()))?;
        let alt = config
            .interface_alt_settings()
            .find(|a| a.class() == VENDOR_CLASS)
            .ok_or_else(|| Error::Usb("Camera has no vendor interface".into()))?;
        let (mut ep_out, mut ep_in) = (None, None);
        for ep in alt.endpoints() {
            use nusb::descriptors::TransferType;
            use nusb::transfer::Direction;
            match (ep.transfer_type(), ep.direction()) {
                (TransferType::Bulk, Direction::Out) => ep_out = ep_out.or(Some(ep.address())),
                (TransferType::Bulk, Direction::In) => ep_in = ep_in.or(Some(ep.address())),
                _ => {}
            }
        }
        let (Some(out_addr), Some(in_addr)) = (ep_out, ep_in) else {
            return Err(Error::Usb("Camera has no bulk endpoints".into()));
        };
        let interface = device
            .claim_interface(alt.interface_number())
            .wait()
            .map_err(|e| Error::Usb(format!("Camera is in use by another program ({})", e)))?;
        let bulk_out = interface.endpoint::<Bulk, Out>(out_addr).map_err(|e| Error::Usb(e.to_string()))?;
        let bulk_in = interface.endpoint::<Bulk, In>(in_addr).map_err(|e| Error::Usb(e.to_string()))?;
        Ok(Self { _interface: interface, bulk_out, bulk_in, protocol, msg_id: 0, pending: Vec::new(), reassembler: Reassembler::default() })
    }

    fn write(&mut self, bytes: &[u8], timeout: Duration) -> Result<()> {
        let mut buf = Buffer::new(bytes.len());
        buf.extend_from_slice(bytes);
        let c = self.bulk_out.transfer_blocking(buf, timeout);
        match c.status {
            Ok(()) => Ok(()),
            Err(nusb::transfer::TransferError::Cancelled) => Err(Error::Timeout),
            Err(e) => Err(Error::Usb(e.to_string())),
        }
    }

    fn read(&mut self, timeout: Duration) -> Result<()> {
        let c = self.bulk_in.transfer_blocking(Buffer::new(64 * 1024), timeout);
        match c.status {
            Ok(()) => {
                self.pending.extend_from_slice(&c.buffer[..c.actual_len.min(c.buffer.len())]);
                Ok(())
            }
            Err(nusb::transfer::TransferError::Cancelled) => Err(Error::Timeout),
            Err(e) => Err(Error::Usb(e.to_string())),
        }
    }

    pub fn send(&mut self, signal: u16, payload: &[u8]) -> Result<()> {
        let m = Message { signal, src: Node::NetHost as u8, dst: route(signal) as u8, payload: payload.to_vec() };
        let timeout = Duration::from_secs(5);
        match self.protocol {
            Protocol::V2 => self.write(&encode_v2(&m), timeout),
            Protocol::V1 => {
                self.msg_id = self.msg_id.wrapping_add(1);
                for f in encode_v1(&m, self.msg_id) {
                    self.write(&f, timeout)?;
                }
                Ok(())
            }
        }
    }

    /// Next message from the camera.
    pub fn receive(&mut self, timeout: Duration) -> Result<Message> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.protocol {
                Protocol::V2 => {
                    if let Some((m, used)) = decode_v2(&self.pending)? {
                        self.pending.drain(..used);
                        return Ok(m);
                    }
                }
                Protocol::V1 => {
                    while self.pending.len() >= 5 && self.pending.len() >= self.pending[4] as usize + 5 {
                        let n = self.pending[4] as usize + 5;
                        let f: Vec<u8> = self.pending.drain(..n).collect();
                        if let Some(m) = self.reassembler.push(&f)? {
                            return Ok(m);
                        }
                    }
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout);
            }
            self.read(left)?;
        }
    }

    /// Sends a request and waits for `response`, skipping unrelated events.
    pub fn request(&mut self, signal: u16, payload: &[u8], response: u16) -> Result<Vec<u8>> {
        self.send(signal, payload)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let m = self.receive(deadline.saturating_duration_since(Instant::now()))?;
            if m.signal == response {
                return Ok(m.payload);
            }
        }
    }

    /// Firmware versions of the main processor and the system controller.
    pub fn firmware(&mut self) -> Result<(String, String)> {
        let text = |b: Vec<u8>| String::from_utf8_lossy(b.split(|&c| c == 0).next().unwrap_or(&[])).trim().to_string();
        let farm = text(self.request(signal::FARM_FIRMWARE_VERSION_REQ, &[0], signal::FARM_FIRMWARE_VERSION_RESP)?);
        let suc = text(self.request(signal::SUC_FIRMWARE_VERSION_REQ, &[0], signal::SUC_FIRMWARE_VERSION_RESP)?);
        Ok((farm, suc))
    }

    pub fn iso(&mut self) -> Result<u32> {
        word(self.request(signal::GET_ISO_REQ, &[0], signal::GET_ISO_RESP)?)
    }

    pub fn set_iso(&mut self, iso: u32) -> Result<()> {
        status(self.request(signal::SET_ISO_REQ, &iso.to_le_bytes(), signal::SET_ISO_RESP)?)
    }

    pub fn exposures_left(&mut self) -> Result<u32> {
        word(self.request(signal::EXPOSURES_LEFT_REQ, &[0], signal::EXPOSURES_LEFT_RESP)?)
    }

    /// Releases the shutter and waits until the sensor read-out is done.
    pub fn capture(&mut self) -> Result<()> {
        status(self.request(signal::DO_EXPOSURE_REQ, &[0], signal::DO_EXPOSURE_RESP)?)?;
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if self.receive(deadline.saturating_duration_since(Instant::now()))?.signal == signal::EXPOSURE_DONE_EVENT {
                return Ok(());
            }
        }
    }
}

fn word(b: Vec<u8>) -> Result<u32> {
    b.get(..4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).ok_or_else(|| Error::Frame(format!("{}-byte answer", b.len())))
}

fn status(b: Vec<u8>) -> Result<()> {
    match b.first() {
        Some(0) | None => Ok(()),
        Some(c) => Err(Error::Frame(format!("camera refused the request (status {})", c))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(signal: u16, n: usize) -> Message {
        Message { signal, src: Node::NetHost as u8, dst: Node::Farm as u8, payload: (0..n).map(|i| i as u8).collect() }
    }

    #[test]
    fn v2_short_and_long_frames_round_trip() {
        for n in [1, 4, 255, 256, 4000] {
            let m = msg(signal::SET_ISO_REQ, n);
            let mut bytes = encode_v2(&m);
            if n < 256 {
                assert_eq!(bytes[4] as usize, n);
            } else {
                assert_eq!(bytes[4], 0);
            }
            bytes.extend_from_slice(&[9, 9]);
            let (back, used) = decode_v2(&bytes).unwrap().unwrap();
            assert_eq!(back, m);
            assert_eq!(used, bytes.len() - 2);
            assert!(decode_v2(&bytes[..used - 1]).unwrap().is_none());
        }
        assert_eq!(&encode_v2(&msg(0x02ef, 4))[..5], &[0xef, 0x02, 4, 1, 4]);
    }

    #[test]
    fn v1_fragments_reassemble() {
        for n in [0, 1, 246, 247, 1000] {
            let m = msg(signal::DO_EXPOSURE_REQ, n);
            let frags = encode_v1(&m, 7);
            assert_eq!(frags.len(), if n <= 246 { 1 } else { 1 + (n - 246).div_ceil(250) });
            assert!(frags.iter().all(|f| f[4] as usize + 5 == f.len() && f.len() <= 260));
            let mut r = Reassembler::default();
            let mut out = None;
            for f in &frags {
                out = r.push(f).unwrap();
            }
            assert_eq!(out, Some(m));
        }
    }

    #[test]
    fn v1_rejects_out_of_order_fragments() {
        let frags = encode_v1(&msg(1, 600), 1);
        let mut r = Reassembler::default();
        r.push(&frags[0]).unwrap();
        assert!(r.push(&frags[2]).is_err());
        assert!(r.push(&[0; 4]).is_err());
    }

    #[test]
    fn protocol_from_product_id() {
        assert_eq!(Protocol::for_device(VENDOR_HASSELBLAD, 7), Protocol::V2);
        assert_eq!(Protocol::for_device(VENDOR_HASSELBLAD, 30), Protocol::V2);
        assert_eq!(Protocol::for_device(VENDOR_HASSELBLAD, 2), Protocol::V1);
        assert_eq!(Protocol::for_device(VENDOR_DJI, 10), Protocol::V1);
    }
}
