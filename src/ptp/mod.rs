//! Picture Transfer Protocol (ISO 15740) over USB, for tethering and in-camera work.
//!
//! [`Session`] runs PTP transactions over any [`Transport`]; [`usb::UsbTransport`] is the
//! real camera connection. Vendor extensions (Fujifilm RAW conversion, medium-format
//! backs) are built on [`Session::transaction`] in their own modules.

pub mod usb;

use std::time::Duration;

/// Standard operation codes.
pub mod op {
    pub const GET_DEVICE_INFO: u16 = 0x1001;
    pub const OPEN_SESSION: u16 = 0x1002;
    pub const CLOSE_SESSION: u16 = 0x1003;
    pub const GET_STORAGE_IDS: u16 = 0x1004;
    pub const GET_STORAGE_INFO: u16 = 0x1005;
    pub const GET_NUM_OBJECTS: u16 = 0x1006;
    pub const GET_OBJECT_HANDLES: u16 = 0x1007;
    pub const GET_OBJECT_INFO: u16 = 0x1008;
    pub const GET_OBJECT: u16 = 0x1009;
    pub const GET_THUMB: u16 = 0x100A;
    pub const DELETE_OBJECT: u16 = 0x100B;
    pub const SEND_OBJECT_INFO: u16 = 0x100C;
    pub const SEND_OBJECT: u16 = 0x100D;
    pub const INITIATE_CAPTURE: u16 = 0x100E;
    pub const GET_DEVICE_PROP_DESC: u16 = 0x1014;
    pub const GET_DEVICE_PROP_VALUE: u16 = 0x1015;
    pub const SET_DEVICE_PROP_VALUE: u16 = 0x1016;
    pub const GET_PARTIAL_OBJECT: u16 = 0x101B;
}

/// Response codes.
pub mod rc {
    pub const OK: u16 = 0x2001;
    pub const GENERAL_ERROR: u16 = 0x2002;
    pub const SESSION_NOT_OPEN: u16 = 0x2003;
    pub const OPERATION_NOT_SUPPORTED: u16 = 0x2005;
    pub const DEVICE_BUSY: u16 = 0x2019;
    pub const SESSION_ALREADY_OPEN: u16 = 0x201E;
}

/// Standard event codes.
pub mod ev {
    pub const OBJECT_ADDED: u16 = 0x4002;
    pub const DEVICE_PROP_CHANGED: u16 = 0x4006;
    pub const CAPTURE_COMPLETE: u16 = 0x400D;
}

const COMMAND: u16 = 1;
const DATA: u16 = 2;
const RESPONSE: u16 = 3;
const EVENT: u16 = 4;
const HEADER: usize = 12;

/// Largest data phase accepted from a camera (a 400 MP Pixel Shift RAF is ~1 GB).
pub const MAX_DATA: usize = 2 * 1024 * 1024 * 1024 - 1;

#[derive(Debug)]
pub enum Error {
    Usb(String),
    Timeout,
    /// The camera answered with a response code other than OK.
    Response { op: u16, code: u16 },
    Protocol(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usb(e) => write!(f, "USB error: {}", e),
            Self::Timeout => write!(f, "Camera did not answer in time"),
            Self::Response { op, code } => write!(f, "Camera refused operation 0x{:04X}: {}", op, response_name(*code)),
            Self::Protocol(e) => write!(f, "PTP protocol error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub fn response_name(code: u16) -> String {
    match code {
        0x2001 => "OK".into(),
        0x2002 => "General error".into(),
        0x2003 => "Session not open".into(),
        0x2005 => "Operation not supported".into(),
        0x2006 => "Parameter not supported".into(),
        0x2007 => "Incomplete transfer".into(),
        0x2009 => "Invalid object handle".into(),
        0x200A => "Device property not supported".into(),
        0x200C => "Store full".into(),
        0x2019 => "Device busy".into(),
        0x201E => "Session already open".into(),
        c => format!("0x{:04X}", c),
    }
}

/// Moves raw PTP containers to and from the camera.
pub trait Transport {
    /// Sends one whole container.
    fn send(&mut self, container: &[u8], timeout: Duration) -> Result<()>;
    /// Receives one whole container.
    fn receive(&mut self, timeout: Duration) -> Result<Vec<u8>>;
    /// Receives an event container, or None if none arrives within `timeout`.
    fn event(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>>;
}

/// Builds a container: length, type, code, transaction id, then params or payload.
pub fn container(kind: u16, code: u16, transaction: u32, params: &[u32], payload: &[u8]) -> Vec<u8> {
    let len = HEADER + params.len() * 4 + payload.len();
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&(len.min(u32::MAX as usize) as u32).to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&code.to_le_bytes());
    out.extend_from_slice(&transaction.to_le_bytes());
    for p in params {
        out.extend_from_slice(&p.to_le_bytes());
    }
    out.extend_from_slice(payload);
    out
}

/// (type, code, transaction id, rest) of a container.
fn split(raw: &[u8]) -> Result<(u16, u16, u32, &[u8])> {
    if raw.len() < HEADER {
        return Err(Error::Protocol(format!("container of {} bytes", raw.len())));
    }
    let kind = u16::from_le_bytes([raw[4], raw[5]]);
    let code = u16::from_le_bytes([raw[6], raw[7]]);
    let tid = u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]);
    Ok((kind, code, tid, &raw[HEADER..]))
}

fn params_of(rest: &[u8]) -> Vec<u32> {
    rest.as_chunks::<4>().0.iter().take(5).map(|c| u32::from_le_bytes(*c)).collect()
}

/// Result of a transaction: response parameters and the data phase, if any.
#[derive(Debug, Default)]
pub struct Response {
    pub params: Vec<u32>,
    pub data: Vec<u8>,
}

/// A camera event (ObjectAdded, DevicePropChanged...).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub code: u16,
    pub params: Vec<u32>,
}

/// GetDeviceInfo, the parts that matter for choosing features.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DeviceInfo {
    pub vendor_extension_id: u32,
    pub vendor_extension: String,
    pub operations: Vec<u16>,
    pub events: Vec<u16>,
    pub properties: Vec<u16>,
    pub manufacturer: String,
    pub model: String,
    pub version: String,
    pub serial: String,
}

impl DeviceInfo {
    pub fn supports(&self, op: u16) -> bool {
        self.operations.contains(&op)
    }

    pub fn has_property(&self, prop: u16) -> bool {
        self.properties.contains(&prop)
    }
}

/// Little-endian reader for PTP datasets. Reads past the end yield errors, never panics.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self.data.get(self.pos..self.pos + n).ok_or_else(|| Error::Protocol("dataset truncated".into()))?;
        self.pos += n;
        Ok(s)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap_or_default()))
    }

    pub fn u16_array(&mut self) -> Result<Vec<u16>> {
        let n = self.u32()? as usize;
        if n > (self.data.len() - self.pos) / 2 {
            return Err(Error::Protocol("array longer than dataset".into()));
        }
        (0..n).map(|_| self.u16()).collect()
    }

    pub fn u32_array(&mut self) -> Result<Vec<u32>> {
        let n = self.u32()? as usize;
        if n > (self.data.len() - self.pos) / 4 {
            return Err(Error::Protocol("array longer than dataset".into()));
        }
        (0..n).map(|_| self.u32()).collect()
    }

    /// PTP string: u8 character count (including the terminating NUL), then UCS-2LE.
    pub fn string(&mut self) -> Result<String> {
        let n = self.u8()? as usize;
        let units: Vec<u16> = self.take(n * 2)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        Ok(String::from_utf16_lossy(&units).trim_end_matches('\0').to_string())
    }

    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos.min(self.data.len())..]
    }
}

/// Encodes a PTP string (at most 254 characters).
pub fn string(s: &str) -> Vec<u8> {
    let units: Vec<u16> = s.encode_utf16().take(254).collect();
    if units.is_empty() {
        return vec![0];
    }
    let mut out = vec![(units.len() + 1) as u8];
    for u in units.iter().chain(std::iter::once(&0)) {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

pub fn parse_device_info(data: &[u8]) -> Result<DeviceInfo> {
    let mut r = Reader::new(data);
    let _standard_version = r.u16()?;
    let vendor_extension_id = r.u32()?;
    let _vendor_extension_version = r.u16()?;
    let vendor_extension = r.string()?;
    let _functional_mode = r.u16()?;
    let operations = r.u16_array()?;
    let events = r.u16_array()?;
    let properties = r.u16_array()?;
    let _capture_formats = r.u16_array()?;
    let _image_formats = r.u16_array()?;
    Ok(DeviceInfo {
        vendor_extension_id,
        vendor_extension,
        operations,
        events,
        properties,
        manufacturer: r.string()?,
        model: r.string()?,
        version: r.string()?,
        serial: r.string().unwrap_or_default(),
    })
}

/// ObjectInfo dataset, the fields tethering and import use.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ObjectInfo {
    pub storage_id: u32,
    pub format: u16,
    pub size: u32,
    pub width: u32,
    pub height: u32,
    pub parent: u32,
    pub filename: String,
    pub capture_date: String,
}

pub fn parse_object_info(data: &[u8]) -> Result<ObjectInfo> {
    let mut r = Reader::new(data);
    let storage_id = r.u32()?;
    let format = r.u16()?;
    let _protection = r.u16()?;
    let size = r.u32()?;
    let _thumb_format = r.u16()?;
    let _thumb_size = r.u32()?;
    let _thumb_w = r.u32()?;
    let _thumb_h = r.u32()?;
    let width = r.u32()?;
    let height = r.u32()?;
    let _depth = r.u32()?;
    let parent = r.u32()?;
    let _assoc_type = r.u16()?;
    let _assoc_desc = r.u32()?;
    let _seq = r.u32()?;
    let filename = r.string()?;
    let capture_date = r.string().unwrap_or_default();
    Ok(ObjectInfo { storage_id, format, size, width, height, parent, filename, capture_date })
}

/// Builds an ObjectInfo dataset for SendObjectInfo.
pub fn object_info(format: u16, size: u32, filename: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u32.to_le_bytes()); // StorageID
    out.extend_from_slice(&format.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // ProtectionStatus
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // ThumbFormat
    for _ in 0..7 {
        // ThumbCompressedSize, ThumbPixWidth/Height, ImagePixWidth/Height, BitDepth, ParentObject
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    out.extend_from_slice(&0u16.to_le_bytes()); // AssociationType
    out.extend_from_slice(&0u32.to_le_bytes()); // AssociationDesc
    out.extend_from_slice(&0u32.to_le_bytes()); // SequenceNumber
    out.extend_from_slice(&string(filename));
    out.extend_from_slice(&[0, 0, 0]); // CaptureDate, ModificationDate, Keywords
    out
}

/// An open PTP session.
pub struct Session<T: Transport> {
    transport: T,
    transaction: u32,
    pub timeout: Duration,
}

impl<T: Transport> Session<T> {
    /// Opens session 1, closing a stale one left by a crashed program first.
    pub fn open(transport: T) -> Result<Self> {
        let mut s = Self { transport, transaction: 0, timeout: Duration::from_secs(5) };
        match s.transaction(op::OPEN_SESSION, &[1], None) {
            Err(Error::Response { code: rc::SESSION_ALREADY_OPEN, .. }) => {
                let _ = s.transaction(op::CLOSE_SESSION, &[], None);
                s.transaction = 0;
                s.transaction(op::OPEN_SESSION, &[1], None)?;
            }
            r => {
                r?;
            }
        }
        Ok(s)
    }

    /// Runs one transaction: command, optional data out, optional data in, response.
    /// A response code other than OK is returned as [`Error::Response`].
    pub fn transaction(&mut self, code: u16, params: &[u32], data_out: Option<&[u8]>) -> Result<Response> {
        self.transaction_timeout(code, params, data_out, self.timeout)
    }

    pub fn transaction_timeout(&mut self, code: u16, params: &[u32], data_out: Option<&[u8]>, timeout: Duration) -> Result<Response> {
        // OpenSession uses transaction id 0
        let tid = if code == op::OPEN_SESSION { 0 } else { self.next_tid() };
        self.transport.send(&container(COMMAND, code, tid, &params[..params.len().min(5)], &[]), timeout)?;
        if let Some(payload) = data_out {
            self.transport.send(&container(DATA, code, tid, &[], payload), timeout)?;
        }
        let mut data = Vec::new();
        loop {
            let raw = self.transport.receive(timeout)?;
            let (kind, rcode, rtid, rest) = split(&raw)?;
            match kind {
                DATA if rtid == tid => data = rest.to_vec(),
                RESPONSE if rtid == tid || code == op::OPEN_SESSION => {
                    if rcode != rc::OK {
                        return Err(Error::Response { op: code, code: rcode });
                    }
                    return Ok(Response { params: params_of(rest), data });
                }
                // Leftovers of an aborted transaction: skip them
                DATA | RESPONSE => continue,
                other => return Err(Error::Protocol(format!("unexpected container type {}", other))),
            }
        }
    }

    fn next_tid(&mut self) -> u32 {
        self.transaction = self.transaction.wrapping_add(1).max(1);
        self.transaction
    }

    pub fn device_info(&mut self) -> Result<DeviceInfo> {
        parse_device_info(&self.transaction(op::GET_DEVICE_INFO, &[], None)?.data)
    }

    pub fn storage_ids(&mut self) -> Result<Vec<u32>> {
        Reader::new(&self.transaction(op::GET_STORAGE_IDS, &[], None)?.data).u32_array()
    }

    /// Object handles in a storage (0xFFFFFFFF = all), optionally of one format.
    pub fn object_handles(&mut self, storage: u32, format: u16, parent: u32) -> Result<Vec<u32>> {
        Reader::new(&self.transaction(op::GET_OBJECT_HANDLES, &[storage, format as u32, parent], None)?.data).u32_array()
    }

    pub fn object_info(&mut self, handle: u32) -> Result<ObjectInfo> {
        parse_object_info(&self.transaction(op::GET_OBJECT_INFO, &[handle], None)?.data)
    }

    /// Downloads an object (a photo); large files get a long timeout.
    pub fn object(&mut self, handle: u32) -> Result<Vec<u8>> {
        Ok(self.transaction_timeout(op::GET_OBJECT, &[handle], None, Duration::from_secs(120))?.data)
    }

    pub fn delete_object(&mut self, handle: u32) -> Result<()> {
        self.transaction(op::DELETE_OBJECT, &[handle, 0], None).map(|_| ())
    }

    pub fn initiate_capture(&mut self) -> Result<()> {
        self.transaction_timeout(op::INITIATE_CAPTURE, &[0, 0], None, Duration::from_secs(30)).map(|_| ())
    }

    pub fn prop_value(&mut self, prop: u16) -> Result<Vec<u8>> {
        Ok(self.transaction(op::GET_DEVICE_PROP_VALUE, &[prop as u32], None)?.data)
    }

    pub fn set_prop_value(&mut self, prop: u16, value: &[u8]) -> Result<()> {
        self.transaction(op::SET_DEVICE_PROP_VALUE, &[prop as u32], Some(value)).map(|_| ())
    }

    pub fn prop_u16(&mut self, prop: u16) -> Result<u16> {
        Reader::new(&self.prop_value(prop)?).u16()
    }

    pub fn set_prop_u16(&mut self, prop: u16, value: u16) -> Result<()> {
        self.set_prop_value(prop, &value.to_le_bytes())
    }

    pub fn prop_desc(&mut self, prop: u16) -> Result<Vec<u8>> {
        Ok(self.transaction(op::GET_DEVICE_PROP_DESC, &[prop as u32], None)?.data)
    }

    /// Next event, if one arrives within `timeout`.
    pub fn event(&mut self, timeout: Duration) -> Result<Option<Event>> {
        let Some(raw) = self.transport.event(timeout)? else { return Ok(None) };
        let (kind, code, _, rest) = split(&raw)?;
        if kind != EVENT {
            return Err(Error::Protocol(format!("container type {} on the event pipe", kind)));
        }
        Ok(Some(Event { code, params: params_of(rest) }))
    }

    /// Closes the session and returns the transport.
    pub fn close(mut self) -> T {
        let _ = self.transaction(op::CLOSE_SESSION, &[], None);
        self.transport
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Scripted camera: records what was sent and replays canned containers.
    #[derive(Default)]
    pub struct Mock {
        pub sent: Vec<Vec<u8>>,
        pub replies: VecDeque<Vec<u8>>,
        pub events: VecDeque<Vec<u8>>,
    }

    impl Mock {
        pub fn reply(&mut self, code: u16, tid: u32, data: Option<&[u8]>, params: &[u32]) {
            if let Some(d) = data {
                self.replies.push_back(container(DATA, code, tid, &[], d));
            }
            self.replies.push_back(container(RESPONSE, rc::OK, tid, params, &[]));
        }
    }

    impl Transport for Mock {
        fn send(&mut self, c: &[u8], _: Duration) -> Result<()> {
            self.sent.push(c.to_vec());
            Ok(())
        }
        fn receive(&mut self, _: Duration) -> Result<Vec<u8>> {
            self.replies.pop_front().ok_or(Error::Timeout)
        }
        fn event(&mut self, _: Duration) -> Result<Option<Vec<u8>>> {
            Ok(self.events.pop_front())
        }
    }

    #[test]
    fn containers_round_trip() {
        let c = container(COMMAND, op::GET_OBJECT, 7, &[0x10, 0x20], &[]);
        assert_eq!(c.len(), 20);
        let (kind, code, tid, rest) = split(&c).unwrap();
        assert_eq!((kind, code, tid, params_of(rest)), (COMMAND, op::GET_OBJECT, 7, vec![0x10, 0x20]));
    }

    #[test]
    fn strings_round_trip() {
        let enc = string("X-T30 ğ");
        assert_eq!(enc[0], 8);
        assert_eq!(Reader::new(&enc).string().unwrap(), "X-T30 ğ");
        assert_eq!(string(""), vec![0]);
    }

    #[test]
    fn session_runs_transactions_with_data_phases() {
        let mut m = Mock::default();
        m.reply(op::OPEN_SESSION, 0, None, &[]);
        let mut info = Vec::new();
        info.extend_from_slice(&100u16.to_le_bytes());
        info.extend_from_slice(&6u32.to_le_bytes());
        info.extend_from_slice(&100u16.to_le_bytes());
        info.extend_from_slice(&string("fujifilm.co.jp: 1.0;"));
        info.extend_from_slice(&0u16.to_le_bytes());
        for arr in [&[0x1001u16, 0x900C][..], &[0x4002], &[0xD185], &[], &[]] {
            info.extend_from_slice(&(arr.len() as u32).to_le_bytes());
            for v in arr {
                info.extend_from_slice(&v.to_le_bytes());
            }
        }
        for s in ["FUJIFILM", "X-T30", "1.00", "1234"] {
            info.extend_from_slice(&string(s));
        }
        m.reply(op::GET_DEVICE_INFO, 1, Some(&info), &[]);
        m.reply(op::SET_DEVICE_PROP_VALUE, 2, None, &[]);
        m.events.push_back(container(EVENT, ev::OBJECT_ADDED, 0, &[0x42], &[]));

        let mut s = Session::open(m).unwrap();
        let di = s.device_info().unwrap();
        assert_eq!((di.model.as_str(), di.supports(0x900C), di.has_property(0xD185)), ("X-T30", true, true));
        s.set_prop_u16(0xD183, 0).unwrap();
        assert_eq!(s.event(Duration::ZERO).unwrap(), Some(Event { code: ev::OBJECT_ADDED, params: vec![0x42] }));

        let m = s.close();
        // command, command, command + data, close command
        assert_eq!(m.sent.len(), 5);
        let (kind, code, tid, rest) = split(&m.sent[3]).unwrap();
        assert_eq!((kind, code, tid, rest), (DATA, op::SET_DEVICE_PROP_VALUE, 2, &[0u8, 0][..]));
    }

    #[test]
    fn refused_operations_are_errors() {
        let mut m = Mock::default();
        m.reply(op::OPEN_SESSION, 0, None, &[]);
        m.replies.push_back(container(RESPONSE, rc::DEVICE_BUSY, 1, &[], &[]));
        let mut s = Session::open(m).unwrap();
        match s.prop_value(0xD185) {
            Err(Error::Response { code, .. }) => assert_eq!(code, rc::DEVICE_BUSY),
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn truncated_datasets_do_not_panic() {
        assert!(parse_device_info(&[1, 2, 3]).is_err());
        assert!(Reader::new(&[0xff, 0xff, 0xff, 0x7f]).u16_array().is_err());
        assert!(parse_object_info(&[0; 10]).is_err());
    }
}
