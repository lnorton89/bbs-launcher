//! Built-in Bluetti power-station monitor screen.
//!
//! Connects to the MQTT broker that `bluetti-mqtt-node` (or the Python
//! `bluetti_mqtt`) publishes device state to, subscribes to
//! `<prefix>/state/#`, and renders the live values. Topics look like
//! `bluetti/state/AC500-2237000003358/total_battery_percent` with plain
//! string payloads (`33`, `ON`, `AC500`).
//!
//! The MQTT layer is a deliberately small hand-rolled 3.1.1 subscriber —
//! connect, subscribe, receive QoS 0/1 publishes, keepalive pings — so
//! the app needs no MQTT client crate or async runtime. All socket work
//! happens on a background thread that posts parsed updates through an
//! mpsc channel; the main loop drains it every tick.

use crate::config::BluettiConfig;
use ratatui::{layout::Rect, widgets::ListState};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long between reconnect attempts after a dropped session.
const RECONNECT_DELAY: Duration = Duration::from_secs(3);

/// Socket read timeout; doubles as the idle interval between keepalive
/// pings, so it must stay comfortably under the CONNECT keepalive (60s).
const READ_TIMEOUT: Duration = Duration::from_secs(20);

/// A value older than this renders dimmed, so a wedged bridge is
/// visible as stale data rather than lying with confident numbers.
pub const STALE_AFTER: Duration = Duration::from_secs(60);

/// Messages from the subscriber thread back to the UI.
struct Msg {
    /// Subscriber-thread generation; stale threads' messages (after a
    /// reconnect or config reload) are discarded by `poll`.
    gen: u64,
    kind: MsgKind,
}

enum MsgKind {
    Connected,
    Disconnected(String),
    /// A raw state publish; the view splits the topic against its
    /// configured prefix (the socket thread doesn't know it).
    Publish { topic: String, value: String },
}

/// One live field value and when it last changed.
#[derive(Debug, Clone)]
pub struct Field {
    pub value: String,
    pub updated: Instant,
}

/// All the mutable state behind the Bluetti screen.
#[derive(Debug)]
pub struct BluettiView {
    /// Broker as configured, for display ("mqtt://127.0.0.1:1883").
    pub broker: String,
    /// Broker as dialled ("127.0.0.1:1883").
    addr: String,
    prefix: String,
    /// Only show this device id when configured.
    pub device_filter: Option<String>,
    /// Discovered device ids, in order of first appearance.
    pub devices: Vec<String>,
    /// device id -> field name -> live value.
    pub fields: BTreeMap<String, BTreeMap<String, Field>>,
    pub tab: usize,
    pub state: ListState,
    pub status: String,
    pub connected: bool,
    pub msg_count: u64,
    pub last_msg: Option<Instant>,
    /// Where the list pane was last drawn, for mouse hit-testing.
    pub list_area: Option<Rect>,
    started: bool,
    generation: u64,
    shutdown: Arc<AtomicBool>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl BluettiView {
    pub fn new(config: Option<BluettiConfig>) -> Self {
        let cfg = config.unwrap_or_default();
        let broker = cfg
            .broker
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| "mqtt://127.0.0.1:1883".into());
        let addr = broker_addr(&broker);
        let prefix = cfg
            .topic_prefix
            .map(|p| p.trim_matches('/').to_string())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "bluetti".into());
        let device_filter = cfg.device.filter(|d| !d.trim().is_empty());
        let (tx, rx) = channel();
        BluettiView {
            broker,
            addr,
            prefix,
            device_filter,
            devices: Vec::new(),
            fields: BTreeMap::new(),
            tab: 0,
            state: ListState::default(),
            status: "not connected".into(),
            connected: false,
            msg_count: 0,
            last_msg: None,
            list_area: None,
            started: false,
            generation: 0,
            shutdown: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
        }
    }

    /// Called when the screen is opened. Starts the subscriber on first
    /// open; later opens just land on whatever is already streaming in.
    pub fn open(&mut self) {
        if !self.started {
            self.start_thread();
        }
        if self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    /// Tears down the current subscriber (if any) and dials again.
    pub fn reconnect(&mut self) {
        self.status = format!("reconnecting to {}…", self.broker);
        self.connected = false;
        self.start_thread();
    }

    fn start_thread(&mut self) {
        // Stop the previous thread and invalidate its in-flight sends.
        self.shutdown.store(true, Ordering::Relaxed);
        self.shutdown = Arc::new(AtomicBool::new(false));
        self.generation = self.generation.wrapping_add(1);
        self.started = true;

        let addr = self.addr.clone();
        let topic = format!("{}/state/#", self.prefix);
        let tx = self.tx.clone();
        let gen = self.generation;
        let shutdown = Arc::clone(&self.shutdown);
        std::thread::spawn(move || subscriber_loop(&addr, &topic, gen, &tx, &shutdown));
    }

    /// Signals the subscriber thread to exit (it notices at the next
    /// read timeout at the latest). Used when a config reload replaces
    /// the view.
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    /// Drains subscriber messages. Called on every tick — cheap when
    /// idle, and keeps the screen current even while it isn't open.
    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            if msg.gen != self.generation {
                continue;
            }
            match msg.kind {
                MsgKind::Connected => {
                    self.connected = true;
                    self.status = format!("connected to {}", self.broker);
                }
                MsgKind::Disconnected(err) => {
                    self.connected = false;
                    self.status = err;
                }
                MsgKind::Publish { topic, value } => {
                    let Some((device, field)) = split_state_topic(&topic, &self.prefix)
                    else {
                        continue;
                    };
                    // The raw JSON snapshot duplicates every field.
                    if field == "_raw" {
                        continue;
                    }
                    if self.device_filter.as_deref().is_some_and(|f| f != device) {
                        continue;
                    }
                    let (device, field) = (device.to_string(), field.to_string());
                    if !self.devices.contains(&device) {
                        self.devices.push(device.clone());
                    }
                    self.msg_count += 1;
                    self.last_msg = Some(Instant::now());
                    self.fields.entry(device).or_default().insert(
                        field,
                        Field {
                            value,
                            updated: Instant::now(),
                        },
                    );
                }
            }
        }
    }

    /// The device id the current tab shows.
    pub fn current_device(&self) -> Option<&str> {
        self.devices.get(self.tab).map(|s| s.as_str())
    }

    /// The current device's fields in display order: the well-known
    /// stats first (battery, power flows, switches), then anything the
    /// bridge publishes that we don't know, then identity/firmware.
    pub fn sorted_fields(&self) -> Vec<(&str, &Field)> {
        let Some(device) = self.current_device() else {
            return Vec::new();
        };
        let Some(fields) = self.fields.get(device) else {
            return Vec::new();
        };
        let mut out: Vec<(&str, &Field)> =
            fields.iter().map(|(k, v)| (k.as_str(), v)).collect();
        out.sort_by_key(|(name, _)| field_rank(name));
        out
    }

    pub fn next(&mut self) {
        self.move_selection(1);
    }

    pub fn previous(&mut self) {
        self.move_selection(-1);
    }

    fn move_selection(&mut self, delta: i64) {
        let n = self.sorted_fields().len();
        if n == 0 {
            return;
        }
        let cur = self.state.selected().unwrap_or(0) as i64;
        let next = (cur + delta).clamp(0, n as i64 - 1) as usize;
        self.state.select(Some(next));
    }

    pub fn next_tab(&mut self) {
        if !self.devices.is_empty() {
            self.tab = (self.tab + 1) % self.devices.len();
            self.state.select(Some(0));
        }
    }

    pub fn prev_tab(&mut self) {
        if !self.devices.is_empty() {
            self.tab = (self.tab + self.devices.len() - 1) % self.devices.len();
            self.state.select(Some(0));
        }
    }
}

// ───────────────────────── field presentation ─────────────────────────

/// Well-known fields in the order the list should show them; anything
/// unknown slots between the stats and the identity block.
const FIELD_ORDER: [&str; 12] = [
    "total_battery_percent",
    "ac_output_power",
    "dc_output_power",
    "ac_input_power",
    "dc_input_power",
    "power_generation",
    "ac_output_on",
    "dc_output_on",
    "device_type",
    "serial_number",
    "arm_version",
    "dsp_version",
];

/// Where identity/firmware rows start in [`FIELD_ORDER`].
const META_FIELDS_AT: usize = 8;

fn field_rank(name: &str) -> usize {
    FIELD_ORDER
        .iter()
        .position(|f| *f == name)
        // Unknown fields sort after the stats but before the meta rows,
        // biased by nothing else (BTreeMap iteration keeps them alpha).
        .map(|i| if i < META_FIELDS_AT { i } else { i + 100 })
        .unwrap_or(50)
}

pub fn field_label(name: &str) -> String {
    match name {
        "total_battery_percent" => "Battery".into(),
        "ac_output_power" => "AC output".into(),
        "dc_output_power" => "DC output".into(),
        "ac_input_power" => "AC input".into(),
        "dc_input_power" => "DC input".into(),
        "power_generation" => "Total generated".into(),
        "ac_output_on" => "AC output switch".into(),
        "dc_output_on" => "DC output switch".into(),
        "device_type" => "Model".into(),
        "serial_number" => "Serial".into(),
        "arm_version" => "ARM firmware".into(),
        "dsp_version" => "DSP firmware".into(),
        other => {
            // snake_case -> Sentence case for fields we don't know.
            let mut s = other.replace('_', " ");
            if let Some(first) = s.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            s
        }
    }
}

/// Unit suffix shown after the value, when the field has one.
pub fn field_unit(name: &str) -> &'static str {
    match name {
        "total_battery_percent" => " %",
        "ac_output_power" | "dc_output_power" | "ac_input_power" | "dc_input_power" => " W",
        "power_generation" => " kWh",
        _ => "",
    }
}

/// `mqtt://host:port` / `host:port` / `host` -> dialable `host:port`.
fn broker_addr(broker: &str) -> String {
    let stripped = broker
        .trim()
        .strip_prefix("mqtt://")
        .or_else(|| broker.trim().strip_prefix("tcp://"))
        .unwrap_or(broker.trim());
    let stripped = stripped.trim_end_matches('/');
    if stripped.contains(':') {
        stripped.to_string()
    } else {
        format!("{stripped}:1883")
    }
}

/// `bluetti/state/<device>/<field>` -> (device, field).
fn split_state_topic<'a>(topic: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let rest = topic
        .strip_prefix(prefix)?
        .strip_prefix('/')?
        .strip_prefix("state/")?;
    let (device, field) = rest.split_once('/')?;
    (!device.is_empty() && !field.is_empty() && !field.contains('/'))
        .then_some((device, field))
}

// ───────────────────────── subscriber thread ─────────────────────────

fn subscriber_loop(
    addr: &str,
    topic: &str,
    gen: u64,
    tx: &Sender<Msg>,
    shutdown: &AtomicBool,
) {
    while !shutdown.load(Ordering::Relaxed) {
        let err = match run_session(addr, topic, gen, tx, shutdown) {
            Ok(()) => return, // clean shutdown
            Err(e) => e,
        };
        let _ = tx.send(Msg {
            gen,
            kind: MsgKind::Disconnected(format!(
                "broker unreachable ({err}) — retrying every {}s",
                RECONNECT_DELAY.as_secs()
            )),
        });
        // Sleep in small slices so a shutdown isn't held up.
        let waited = Instant::now();
        while waited.elapsed() < RECONNECT_DELAY {
            if shutdown.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

/// One broker session: connect, subscribe, then pump publishes until
/// the socket dies or shutdown is requested.
fn run_session(
    addr: &str,
    topic: &str,
    gen: u64,
    tx: &Sender<Msg>,
    shutdown: &AtomicBool,
) -> io::Result<()> {
    let sock_addr = addr
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address"))?;
    let mut stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_nodelay(true).ok();

    let client_id = format!("bbs-launcher-{}", std::process::id());
    stream.write_all(&encode_connect(&client_id))?;
    let (header, body) = read_packet(&mut stream)?;
    if header >> 4 != 2 || body.len() < 2 || body[1] != 0 {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            format!("broker refused connection (CONNACK {:?})", body.get(1)),
        ));
    }
    stream.write_all(&encode_subscribe(topic))?;
    let _ = tx.send(Msg {
        gen,
        kind: MsgKind::Connected,
    });

    loop {
        if shutdown.load(Ordering::Relaxed) {
            return Ok(());
        }
        match read_packet(&mut stream) {
            Ok((header, body)) => match header >> 4 {
                // PUBLISH
                3 => {
                    if let Some((topic, payload, pid)) = parse_publish(header, &body) {
                        // QoS 1 must be acknowledged or the broker
                        // redelivers forever.
                        if let Some(pid) = pid {
                            stream.write_all(&encode_puback(pid))?;
                        }
                        let _ = tx.send(Msg {
                            gen,
                            kind: MsgKind::Publish {
                                topic,
                                value: String::from_utf8_lossy(&payload).into_owned(),
                            },
                        });
                    }
                }
                // SUBACK / PINGRESP — nothing to do.
                9 | 13 => {}
                _ => {}
            },
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                // Idle: keep the session alive.
                stream.write_all(&encode_pingreq())?;
            }
            Err(e) => return Err(e),
        }
    }
}

// ────────────────────────── mqtt 3.1.1 codec ──────────────────────────

/// Wraps a packet body in the fixed header + varint remaining-length.
fn packet(header: u8, body: Vec<u8>) -> Vec<u8> {
    let mut out = vec![header];
    let mut n = body.len();
    loop {
        let mut b = (n % 128) as u8;
        n /= 128;
        if n > 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
    out.extend(body);
    out
}

fn push_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u16).to_be_bytes());
    buf.extend_from_slice(s.as_bytes());
}

fn encode_connect(client_id: &str) -> Vec<u8> {
    let mut body = Vec::new();
    push_str(&mut body, "MQTT");
    body.push(0x04); // protocol level 4 = 3.1.1
    body.push(0x02); // clean session
    body.extend_from_slice(&60u16.to_be_bytes()); // keepalive secs
    push_str(&mut body, client_id);
    packet(0x10, body)
}

fn encode_subscribe(topic: &str) -> Vec<u8> {
    let mut body = vec![0x00, 0x01]; // packet id 1
    push_str(&mut body, topic);
    body.push(0x00); // QoS 0
    packet(0x82, body)
}

fn encode_pingreq() -> Vec<u8> {
    vec![0xC0, 0x00]
}

fn encode_puback(pid: u16) -> Vec<u8> {
    let mut out = vec![0x40, 0x02];
    out.extend_from_slice(&pid.to_be_bytes());
    out
}

/// Reads one packet: (fixed header byte, body).
fn read_packet(stream: &mut TcpStream) -> io::Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 1];
    stream.read_exact(&mut header)?;
    let mut len: usize = 0;
    let mut shift = 0u32;
    loop {
        let mut b = [0u8; 1];
        stream.read_exact(&mut b)?;
        len |= ((b[0] & 0x7F) as usize) << shift;
        if b[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 21 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bad remaining-length varint",
            ));
        }
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;
    Ok((header[0], body))
}

/// Splits a PUBLISH body into (topic, payload, packet id for QoS>0).
fn parse_publish(header: u8, body: &[u8]) -> Option<(String, Vec<u8>, Option<u16>)> {
    let qos = (header >> 1) & 0x03;
    if body.len() < 2 {
        return None;
    }
    let tlen = u16::from_be_bytes([body[0], body[1]]) as usize;
    let mut i = 2 + tlen;
    if body.len() < i {
        return None;
    }
    let topic = String::from_utf8_lossy(&body[2..i]).into_owned();
    let pid = if qos > 0 {
        if body.len() < i + 2 {
            return None;
        }
        let p = u16::from_be_bytes([body[i], body[i + 1]]);
        i += 2;
        Some(p)
    } else {
        None
    };
    Some((topic, body[i..].to_vec(), pid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn broker_addr_accepts_urls_hosts_and_ports() {
        assert_eq!(broker_addr("mqtt://127.0.0.1:1883"), "127.0.0.1:1883");
        assert_eq!(broker_addr("tcp://broker.local:2000/"), "broker.local:2000");
        assert_eq!(broker_addr("192.168.1.5"), "192.168.1.5:1883");
        assert_eq!(broker_addr(" mqtt://host "), "host:1883");
    }

    #[test]
    fn state_topics_split_and_junk_is_rejected() {
        let s = |t| split_state_topic(t, "bluetti");
        assert_eq!(
            s("bluetti/state/AC500-2237000003358/total_battery_percent"),
            Some(("AC500-2237000003358", "total_battery_percent"))
        );
        assert_eq!(s("bluetti/command/AC500-1/ac_output_on"), None);
        assert_eq!(s("other/state/AC500-1/field"), None);
        assert_eq!(s("bluetti/state/AC500-1"), None);
        assert_eq!(s("bluetti/state//field"), None);
        // Deeper topics aren't a device/field pair.
        assert_eq!(s("bluetti/state/AC500-1/a/b"), None);
    }

    #[test]
    fn varint_lengths_roundtrip_through_the_wire_format() {
        // One-byte and multi-byte remaining lengths, decoded by a real
        // socket read on the other end of a local pipe.
        for len in [0usize, 1, 127, 128, 16383, 16384] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let body = vec![0xAB; len];
            let wire = packet(0x30, body.clone());
            let writer = std::thread::spawn(move || {
                let (mut s, _) = listener.accept().unwrap();
                s.write_all(&wire).unwrap();
            });
            let mut stream = TcpStream::connect(addr).unwrap();
            let (header, got) = read_packet(&mut stream).unwrap();
            assert_eq!(header, 0x30);
            assert_eq!(got, body, "len {len}");
            writer.join().unwrap();
        }
    }

    #[test]
    fn publish_packets_parse_for_both_qos_levels() {
        // QoS 0: topic then payload.
        let mut body = Vec::new();
        push_str(&mut body, "bluetti/state/AC500-1/x");
        body.extend_from_slice(b"33");
        let (topic, payload, pid) = parse_publish(0x30, &body).unwrap();
        assert_eq!(topic, "bluetti/state/AC500-1/x");
        assert_eq!(payload, b"33");
        assert_eq!(pid, None);

        // QoS 1: a packet id sits between topic and payload.
        let mut body = Vec::new();
        push_str(&mut body, "t");
        body.extend_from_slice(&7u16.to_be_bytes());
        body.extend_from_slice(b"ON");
        let (topic, payload, pid) = parse_publish(0x32, &body).unwrap();
        assert_eq!(topic, "t");
        assert_eq!(payload, b"ON");
        assert_eq!(pid, Some(7));

        // Truncated bodies fail closed.
        assert!(parse_publish(0x30, &[0x00]).is_none());
        assert!(parse_publish(0x32, &{
            let mut b = Vec::new();
            push_str(&mut b, "t");
            b // missing the packet id
        })
        .is_none());
    }

    #[test]
    fn fields_present_in_a_sensible_order_with_labels_and_units() {
        assert_eq!(field_label("total_battery_percent"), "Battery");
        assert_eq!(field_unit("total_battery_percent"), " %");
        assert_eq!(field_unit("ac_output_power"), " W");
        assert_eq!(field_unit("power_generation"), " kWh");
        assert_eq!(field_label("some_new_field"), "Some new field");
        assert_eq!(field_unit("some_new_field"), "");

        // Battery leads, meta trails, unknown fields sit in between.
        assert!(field_rank("total_battery_percent") < field_rank("ac_output_power"));
        assert!(field_rank("ac_output_on") < field_rank("some_new_field"));
        assert!(field_rank("some_new_field") < field_rank("device_type"));
    }

    /// Minimal in-process broker: accepts one client, checks CONNECT and
    /// SUBSCRIBE, then plays the given publishes (QoS 0 and 1) and
    /// verifies the QoS 1 one is acknowledged.
    fn fake_broker(listener: TcpListener) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let (h, _) = read_packet(&mut s).unwrap();
            assert_eq!(h >> 4, 1, "expected CONNECT first");
            s.write_all(&[0x20, 0x02, 0x00, 0x00]).unwrap(); // CONNACK ok
            let (h, body) = read_packet(&mut s).unwrap();
            assert_eq!(h >> 4, 8, "expected SUBSCRIBE");
            let flt = String::from_utf8_lossy(&body[4..body.len() - 1]).into_owned();
            assert_eq!(flt, "bluetti/state/#");
            s.write_all(&[0x90, 0x03, 0x00, 0x01, 0x00]).unwrap(); // SUBACK

            // One QoS 0 publish…
            let mut b = Vec::new();
            push_str(&mut b, "bluetti/state/AC500-1/total_battery_percent");
            b.extend_from_slice(b"33");
            s.write_all(&packet(0x30, b)).unwrap();
            // …one QoS 1 publish that must be PUBACKed…
            let mut b = Vec::new();
            push_str(&mut b, "bluetti/state/AC500-1/ac_output_on");
            b.extend_from_slice(&9u16.to_be_bytes());
            b.extend_from_slice(b"ON");
            s.write_all(&packet(0x32, b)).unwrap();
            let (h, body) = read_packet(&mut s).unwrap();
            assert_eq!(h, 0x40, "QoS 1 publish must be acknowledged");
            assert_eq!(body, 9u16.to_be_bytes());
            // …and one _raw + one foreign topic the view must ignore.
            let mut b = Vec::new();
            push_str(&mut b, "bluetti/state/AC500-1/_raw");
            b.extend_from_slice(b"{}");
            s.write_all(&packet(0x30, b)).unwrap();
            let mut b = Vec::new();
            push_str(&mut b, "bluetti/command/AC500-1/ac_output_on");
            b.extend_from_slice(b"OFF");
            s.write_all(&packet(0x30, b)).unwrap();
            // Hold the socket open briefly so nothing races the reads.
            std::thread::sleep(Duration::from_millis(300));
        })
    }

    #[test]
    fn subscribes_and_streams_live_values_end_to_end() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let broker = fake_broker(listener);

        let mut view = BluettiView::new(Some(BluettiConfig {
            broker: Some(format!("mqtt://{addr}")),
            device: None,
            topic_prefix: None,
        }));
        view.open();

        // Pump until both fields land or we give up.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            view.poll();
            let n = view
                .fields
                .get("AC500-1")
                .map(|f| f.len())
                .unwrap_or(0);
            if n >= 2 || Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(view.connected, "status: {}", view.status);
        assert_eq!(view.devices, vec!["AC500-1"]);
        let fields = view.fields.get("AC500-1").unwrap();
        assert_eq!(fields["total_battery_percent"].value, "33");
        assert_eq!(fields["ac_output_on"].value, "ON");
        assert!(!fields.contains_key("_raw"), "_raw snapshots are skipped");
        assert_eq!(view.msg_count, 2, "command topics don't count");

        // Ordered for display: battery first.
        let sorted = view.sorted_fields();
        assert_eq!(sorted[0].0, "total_battery_percent");

        view.stop();
        broker.join().unwrap();
    }

    /// End-to-end check against the real local broker. Requires the
    /// bluetti-mqtt-node bridge to be running; run with
    /// `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs the live broker + bridge"]
    fn live_broker_streams_device_state() {
        let mut view = BluettiView::new(None);
        view.open();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            view.poll();
            let populated = view
                .devices
                .first()
                .and_then(|d| view.fields.get(d))
                .is_some_and(|f| f.contains_key("total_battery_percent"));
            if populated || Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(view.connected, "status: {}", view.status);
        let device = view.devices.first().expect("a device should appear").clone();
        let fields = view.fields.get(&device).unwrap();
        println!("device {device}: {} fields", fields.len());
        for (name, field) in view.sorted_fields() {
            println!("  {:<24} {}{}", field_label(name), field.value, field_unit(name));
        }
        assert!(fields.contains_key("total_battery_percent"));
        view.stop();
    }

    #[test]
    fn a_dead_broker_reports_and_a_device_filter_screens() {
        // Nothing listens on this port (bound then dropped).
        let addr = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap()
        };
        let mut view = BluettiView::new(Some(BluettiConfig {
            broker: Some(addr.to_string()),
            device: Some("AC500-1".into()),
            topic_prefix: None,
        }));
        view.open();
        let deadline = Instant::now() + Duration::from_secs(5);
        while view.status == "not connected" && Instant::now() < deadline {
            view.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            view.status.contains("retrying"),
            "unreachable broker surfaces: {}",
            view.status
        );
        view.stop();

        // The device filter drops publishes from other units.
        let msg = |topic: &str| Msg {
            gen: view.generation,
            kind: MsgKind::Publish {
                topic: topic.into(),
                value: "1".into(),
            },
        };
        view.tx.send(msg("bluetti/state/EB3A-9/x")).unwrap();
        view.tx.send(msg("bluetti/state/AC500-1/x")).unwrap();
        view.poll();
        assert_eq!(view.devices, vec!["AC500-1"]);
        assert!(!view.fields.contains_key("EB3A-9"));
    }
}
