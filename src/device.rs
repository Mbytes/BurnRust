//! Device layer: transport abstraction (USB serial / WiFi TCP), a background worker
//! thread that speaks GRBL's character-counting streaming protocol, and a handle the
//! UI uses to drive it without ever blocking the render loop.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

/// GRBL's serial RX buffer size (bytes). We never let in-flight data exceed this.
const RX_BUFFER: usize = 127;
const STATUS_POLL: Duration = Duration::from_millis(200);

/// What we connect to.
#[derive(Debug, Clone)]
pub enum ConnTarget {
    Serial { path: String, baud: u32 },
    Tcp { host: String, port: u16 },
}

/// Commands sent from the UI to the worker.
pub enum Cmd {
    Connect(ConnTarget),
    Disconnect,
    /// A single g-code line (jog, home, manual command), subject to flow control.
    Line(String),
    /// A full job (many lines) streamed with progress tracking.
    Job(Vec<String>),
    /// A single real-time byte sent immediately (`?`, `!`, `~`, 0x18).
    Realtime(u8),
    /// Abort the running job and soft-reset the controller.
    Abort,
}

/// Machine state parsed from a `?` status report.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub state: String,
    pub mpos: Option<(f64, f64, f64)>,
    pub wpos: Option<(f64, f64, f64)>,
}

/// Events emitted by the worker for the UI to consume each frame.
pub enum Evt {
    Connected(String),
    Disconnected,
    ConnectError(String),
    Rx(String),
    Tx(String),
    Status(Status),
    Alarm(String),
    Error(String),
    Progress { done: usize, total: usize },
    JobDone,
}

/// Handle held by the UI.
pub struct DeviceHandle {
    tx: Sender<Cmd>,
    rx: Receiver<Evt>,
    pub connected: bool,
    pub status: Status,
    pub job_total: usize,
    pub job_done: usize,
    pub running: bool,
}

impl DeviceHandle {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Cmd>();
        let (evt_tx, evt_rx) = std::sync::mpsc::channel::<Evt>();
        std::thread::Builder::new()
            .name("grbl-worker".into())
            .spawn(move || worker_loop(cmd_rx, evt_tx))
            .expect("spawn worker");
        Self {
            tx: cmd_tx,
            rx: evt_rx,
            connected: false,
            status: Status::default(),
            job_total: 0,
            job_done: 0,
            running: false,
        }
    }

    pub fn connect(&self, target: ConnTarget) {
        let _ = self.tx.send(Cmd::Connect(target));
    }
    pub fn disconnect(&self) {
        let _ = self.tx.send(Cmd::Disconnect);
    }
    pub fn send_line(&self, line: impl Into<String>) {
        let _ = self.tx.send(Cmd::Line(line.into()));
    }
    pub fn send_job(&self, lines: Vec<String>) {
        let _ = self.tx.send(Cmd::Job(lines));
    }
    pub fn realtime(&self, byte: u8) {
        let _ = self.tx.send(Cmd::Realtime(byte));
    }
    pub fn abort(&self) {
        let _ = self.tx.send(Cmd::Abort);
    }

    // Convenience GRBL commands -------------------------------------------------
    pub fn jog(&self, dx: f64, dy: f64, feed: f64) {
        self.send_line(format!("$J=G91 G21 X{:.3} Y{:.3} F{:.0}", dx, dy, feed));
    }
    pub fn home(&self) {
        self.send_line("$H");
    }
    pub fn set_zero(&self) {
        self.send_line("G10 L20 P1 X0 Y0");
    }
    /// Request all GRBL settings (`$$`); replies arrive as `Evt::Rx` lines `$N=value`.
    pub fn query_settings(&self) {
        self.send_line("$$");
    }
    /// Write a single GRBL setting (`$N=value`).
    pub fn write_setting(&self, num: u32, value: &str) {
        self.send_line(format!("${}={}", num, value));
    }
    pub fn feed_hold(&self) {
        self.realtime(b'!');
    }
    pub fn resume(&self) {
        self.realtime(b'~');
    }
    pub fn soft_reset(&self) {
        self.realtime(0x18);
    }

    /// Drain worker events into local state; returns the events for the UI (console, etc.).
    pub fn poll(&mut self) -> Vec<Evt> {
        let mut out = Vec::new();
        while let Ok(evt) = self.rx.try_recv() {
            match &evt {
                Evt::Connected(_) => {
                    self.connected = true;
                }
                Evt::Disconnected | Evt::ConnectError(_) => {
                    self.connected = false;
                    self.running = false;
                }
                Evt::Status(s) => self.status = s.clone(),
                Evt::Progress { done, total } => {
                    self.job_done = *done;
                    self.job_total = *total;
                    self.running = *done < *total;
                }
                Evt::JobDone => self.running = false,
                _ => {}
            }
            // Track running flag when a job starts.
            if let Evt::Progress { done, total } = &evt {
                if *total > 0 && *done < *total {
                    self.running = true;
                }
            }
            out.push(evt);
        }
        out
    }
}

// --- Transport abstraction ---------------------------------------------------

trait Transport: Send {
    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()>;
    /// Read any immediately-available bytes; returns 0 when none are ready.
    fn read_avail(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
}

struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl Transport for SerialTransport {
    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.port.write_all(data)?;
        self.port.flush()
    }
    fn read_avail(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.port.read(buf) {
            Ok(n) => Ok(n),
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                Ok(0)
            }
            Err(e) => Err(e),
        }
    }
}

struct TcpTransport {
    stream: TcpStream,
}

impl Transport for TcpTransport {
    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.stream.write_all(data)?;
        self.stream.flush()
    }
    fn read_avail(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.stream.read(buf) {
            Ok(n) => Ok(n),
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                Ok(0)
            }
            Err(e) => Err(e),
        }
    }
}

fn open_transport(target: &ConnTarget) -> std::io::Result<Box<dyn Transport>> {
    match target {
        ConnTarget::Serial { path, baud } => {
            let port = serialport::new(path, *baud)
                .timeout(Duration::from_millis(10))
                .open()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
            Ok(Box::new(SerialTransport { port }))
        }
        ConnTarget::Tcp { host, port } => {
            let stream = TcpStream::connect((host.as_str(), *port))?;
            stream.set_read_timeout(Some(Duration::from_millis(10)))?;
            stream.set_nodelay(true).ok();
            Ok(Box::new(TcpTransport { stream }))
        }
    }
}

// --- Worker ------------------------------------------------------------------

struct Worker {
    tx: Sender<Evt>,
    transport: Option<Box<dyn Transport>>,
    send_queue: VecDeque<String>,
    inflight: VecDeque<usize>,
    rx_accum: Vec<u8>,
    job_total: usize,
    job_done: usize,
    job_active: bool,
    last_poll: Instant,
}

impl Worker {
    fn emit(&self, e: Evt) {
        let _ = self.tx.send(e);
    }

    fn disconnect(&mut self) {
        self.transport = None;
        self.send_queue.clear();
        self.inflight.clear();
        self.rx_accum.clear();
        self.job_active = false;
    }

    fn handle_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Connect(target) => {
                self.disconnect();
                match open_transport(&target) {
                    Ok(mut t) => {
                        // Wake GRBL and request a status report.
                        let _ = t.write_all(b"\r\n");
                        self.transport = Some(t);
                        self.last_poll = Instant::now() - STATUS_POLL;
                        let label = match &target {
                            ConnTarget::Serial { path, baud } => format!("{} @ {}", path, baud),
                            ConnTarget::Tcp { host, port } => format!("{}:{}", host, port),
                        };
                        self.emit(Evt::Connected(label));
                    }
                    Err(e) => self.emit(Evt::ConnectError(e.to_string())),
                }
            }
            Cmd::Disconnect => {
                self.disconnect();
                self.emit(Evt::Disconnected);
            }
            Cmd::Line(line) => {
                if self.transport.is_some() {
                    self.send_queue.push_back(sanitize(&line));
                }
            }
            Cmd::Job(lines) => {
                if self.transport.is_some() {
                    self.job_total = lines.len();
                    self.job_done = 0;
                    self.job_active = true;
                    for l in lines {
                        self.send_queue.push_back(sanitize(&l));
                    }
                    self.emit(Evt::Progress {
                        done: 0,
                        total: self.job_total,
                    });
                }
            }
            Cmd::Realtime(byte) => {
                if let Some(t) = self.transport.as_mut() {
                    let _ = t.write_all(&[byte]);
                }
            }
            Cmd::Abort => {
                self.send_queue.clear();
                self.inflight.clear();
                self.job_active = false;
                if let Some(t) = self.transport.as_mut() {
                    let _ = t.write_all(&[0x18]); // soft reset
                }
                self.emit(Evt::JobDone);
            }
        }
    }

    fn pump(&mut self) {
        // 1. Read incoming bytes.
        let mut rx_err = None;
        if let Some(t) = self.transport.as_mut() {
            let mut buf = [0u8; 512];
            match t.read_avail(&mut buf) {
                Ok(n) if n > 0 => self.rx_accum.extend_from_slice(&buf[..n]),
                Ok(_) => {}
                Err(e) => rx_err = Some(e.to_string()),
            }
        }
        if let Some(e) = rx_err {
            self.emit(Evt::Error(format!("E/S: {e}")));
            self.disconnect();
            self.emit(Evt::Disconnected);
            return;
        }

        // 2. Process complete lines.
        while let Some(pos) = self.rx_accum.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = self.rx_accum.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            self.process_line(line);
        }

        // 3. Send queued lines within the RX buffer budget.
        if self.transport.is_some() {
            loop {
                let Some(front) = self.send_queue.front() else {
                    break;
                };
                let need = front.len() + 1;
                let used: usize = self.inflight.iter().sum();
                if used + need > RX_BUFFER {
                    break;
                }
                let line = self.send_queue.pop_front().unwrap();
                let payload = format!("{line}\n");
                if let Some(t) = self.transport.as_mut() {
                    if let Err(e) = t.write_all(payload.as_bytes()) {
                        self.emit(Evt::Error(format!("escritura: {e}")));
                        self.disconnect();
                        self.emit(Evt::Disconnected);
                        return;
                    }
                }
                self.inflight.push_back(need);
                self.emit(Evt::Tx(line));
            }
        }

        // 4. Poll status.
        if self.transport.is_some() && self.last_poll.elapsed() >= STATUS_POLL {
            if let Some(t) = self.transport.as_mut() {
                let _ = t.write_all(b"?");
            }
            self.last_poll = Instant::now();
        }
    }

    fn process_line(&mut self, line: &str) {
        if line.eq_ignore_ascii_case("ok") {
            self.inflight.pop_front();
            if self.job_active {
                self.job_done += 1;
                self.emit(Evt::Progress {
                    done: self.job_done,
                    total: self.job_total,
                });
                if self.job_done >= self.job_total {
                    self.job_active = false;
                    self.emit(Evt::JobDone);
                }
            }
        } else if line.starts_with("error") {
            self.inflight.pop_front();
            self.emit(Evt::Error(line.to_string()));
            if self.job_active {
                self.job_done += 1;
                self.emit(Evt::Progress {
                    done: self.job_done,
                    total: self.job_total,
                });
                if self.job_done >= self.job_total {
                    self.job_active = false;
                    self.emit(Evt::JobDone);
                }
            }
        } else if line.starts_with('<') {
            self.emit(Evt::Status(parse_status(line)));
        } else if line.starts_with("ALARM") {
            self.emit(Evt::Alarm(line.to_string()));
        } else {
            // Welcome banner, [MSG:...], settings echoes, etc.
            self.emit(Evt::Rx(line.to_string()));
        }
    }
}

fn worker_loop(cmd_rx: Receiver<Cmd>, evt_tx: Sender<Evt>) {
    let mut w = Worker {
        tx: evt_tx,
        transport: None,
        send_queue: VecDeque::new(),
        inflight: VecDeque::new(),
        rx_accum: Vec::new(),
        job_total: 0,
        job_done: 0,
        job_active: false,
        last_poll: Instant::now(),
    };

    loop {
        // Drain all pending commands.
        loop {
            match cmd_rx.try_recv() {
                Ok(cmd) => w.handle_cmd(cmd),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        w.pump();
        std::thread::sleep(Duration::from_millis(3));
    }
}

/// Strip inline comments and whitespace from a g-code line before sending.
fn sanitize(line: &str) -> String {
    let no_comment = match line.find(';') {
        Some(i) => &line[..i],
        None => line,
    };
    no_comment.trim().to_string()
}

/// Parse a GRBL status report like `<Idle|MPos:0.000,0.000,0.000|FS:0,0>`.
fn parse_status(line: &str) -> Status {
    let inner = line.trim_start_matches('<').trim_end_matches('>');
    let mut parts = inner.split('|');
    let mut status = Status::default();
    if let Some(state) = parts.next() {
        status.state = state.to_string();
    }
    for field in parts {
        if let Some(rest) = field.strip_prefix("MPos:") {
            status.mpos = parse_triplet(rest);
        } else if let Some(rest) = field.strip_prefix("WPos:") {
            status.wpos = parse_triplet(rest);
        }
    }
    status
}

fn parse_triplet(s: &str) -> Option<(f64, f64, f64)> {
    let mut it = s.split(',').map(|v| v.parse::<f64>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// List available serial ports (best-effort; may be empty without udev).
pub fn list_serial_ports() -> Vec<String> {
    serialport::available_ports()
        .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}
