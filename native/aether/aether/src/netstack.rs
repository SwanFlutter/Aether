use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Checksum, Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::{tcp, udp};
use smoltcp::time::Instant;
use smoltcp::wire::{HardwareAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address, Ipv6Address};
use tokio::sync::{mpsc, oneshot};

use crate::error::{AetherError, Result};

fn tcp_rx_buf() -> usize {
    crate::sysprofile::netstack_tcp_rx_buf_bytes()
}

fn tcp_tx_buf() -> usize {
    crate::sysprofile::netstack_tcp_tx_buf_bytes()
}

fn udp_buf() -> usize {
    crate::sysprofile::netstack_udp_buf_bytes()
}

fn udp_meta() -> usize {
    match crate::sysprofile::tuning().tier {
        crate::sysprofile::Tier::Low => 32,
        crate::sysprofile::Tier::Medium => 64,
        crate::sysprofile::Tier::High => 128,
    }
}

fn app_queue() -> usize {
    crate::sysprofile::channel_capacity()
}

const MAX_INGEST_PER_TICK: usize = 512;
const MAX_RECV_CHUNKS: usize = 128;

/// Per-pass budget for the app->network direction.
///
/// 1.2.3-p1 STARVATION FIX. The loop below is `biased`, so a saturated download
/// took every `select!` wake-up and this direction only ever ran when the
/// download paused. It gets a budget of its own on every pass now instead of
/// competing for a wake-up it could never win.
const MAX_APP_INGEST_PER_TICK: usize = 512;

/// Per-pass budget for control messages (open a flow, close it, set the
/// interface address).
///
/// Small on purpose - a handful of messages per new connection - but it MUST be
/// served under load. A starved `cmd` queue is a tunnel in which no new flow can
/// be opened while an existing one is downloading.
const MAX_CMD_PER_TICK: usize = 64;

const BACKPRESSURE_RETRY: std::time::Duration = std::time::Duration::from_millis(2);
const DROP_REPORT_STEP: usize = 512;
const MAX_IDLE_TICK: std::time::Duration = std::time::Duration::from_millis(250);

/// How much app->network data may sit in ONE flow's overflow queue before that
/// flow is allowed to push back on the shared `data_in` channel.
///
/// 1.2.3-p1 HEAD-OF-LINE FIX. The loop used to hold a single GLOBAL deferred
/// queue and stopped reading `data_in` entirely while it held anything
/// (`recv(), if deferred.is_empty()`). One flow that could not take another byte
/// therefore froze the writes of EVERY other flow through the tunnel, DNS
/// included - which reads as a download that stalls and a page that never opens
/// even though the tunnel is up. The backlog is per-flow and ordered now, and it
/// only gates the shared channel in the extreme case this cap describes.
const MAX_FLOW_BACKLOG_BYTES: usize = 512 * 1024;

/// Outbound packets held back when the WireGuard writer is momentarily behind.
///
/// 1.2.3-p1. `flush_tx` used to TAIL-DROP whatever did not fit, and during a
/// download most of what does not fit is an ACK. Dropping the ACKs of the flow
/// you are trying to speed up makes the remote sender halve its window for no
/// reason at all, and the loss is invisible to any counter in the path. Holding
/// a burst across a couple of 2 ms retries is the point; holding seconds of it
/// would be bufferbloat, so it is bounded rather than unbounded.
const MAX_TX_RETAINED: usize = 256;

/// Keep-alive and dead-peer timeout on every netstack TCP socket.
///
/// Without them a flow whose peer disappears mid-transfer stays `Established`
/// forever, retransmitting into nothing while holding its (now much larger)
/// receive buffer and its slot in the backlog for the whole session.
const TCP_KEEPALIVE: smoltcp::time::Duration = smoltcp::time::Duration::from_secs(15);
const TCP_DEAD_PEER_TIMEOUT: smoltcp::time::Duration = smoltcp::time::Duration::from_secs(90);
const ORPHAN_LINGER: std::time::Duration = std::time::Duration::from_secs(10);

fn max_tcp_pending() -> usize {
    tcp_rx_buf().saturating_mul(2).max(64 * 1024)
}

pub(crate) fn tcp_keepalive() -> std::time::Duration {
    let secs = std::env::var("AETHER_TCP_KEEPALIVE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .map(|v| v.min(86_400))
        .unwrap_or(60);
    std::time::Duration::from_secs(secs)
}

fn tcp_connect_timeout() -> std::time::Duration {
    let secs = std::env::var("AETHER_TCP_CONNECT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .map(|v| v.min(86_400))
        .unwrap_or(30);
    std::time::Duration::from_secs(secs)
}

fn smol_duration(duration: std::time::Duration) -> smoltcp::time::Duration {
    smoltcp::time::Duration::from_millis(duration.as_millis().min(u64::MAX as u128) as u64)
}

#[derive(Debug, Clone, Copy)]
struct TcpLimits {
    connect: std::time::Duration,
    keepalive: std::time::Duration,
    orphan_linger: std::time::Duration,
}

impl TcpLimits {
    fn from_env() -> Self {
        Self {
            connect: tcp_connect_timeout(),
            keepalive: tcp_keepalive(),
            orphan_linger: ORPHAN_LINGER,
        }
    }
}

type OpenTcpResp = oneshot::Sender<std::result::Result<TcpConn, String>>;
type OpenUdpResp = oneshot::Sender<std::result::Result<UdpConn, String>>;

pub struct StackDevice {
    rx: VecDeque<Vec<u8>>,
    tx: VecDeque<Vec<u8>>,
    mtu: usize,
}

impl StackDevice {
    fn new(mtu: usize) -> Self {
        Self {
            rx: VecDeque::new(),
            tx: VecDeque::new(),
            mtu,
        }
    }
}

pub struct StackRxToken(Vec<u8>);
pub struct StackTxToken<'a>(&'a mut VecDeque<Vec<u8>>);

impl RxToken for StackRxToken {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

impl<'a> TxToken for StackTxToken<'a> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut buf = vec![0u8; len];
        let r = f(&mut buf);
        self.0.push_back(buf);
        r
    }
}

impl Device for StackDevice {
    type RxToken<'a> = StackRxToken;
    type TxToken<'a> = StackTxToken<'a>;

    fn receive(&mut self, _t: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let pkt = self.rx.pop_front()?;
        Some((StackRxToken(pkt), StackTxToken(&mut self.tx)))
    }

    fn transmit(&mut self, _t: Instant) -> Option<Self::TxToken<'_>> {
        Some(StackTxToken(&mut self.tx))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ip;
        caps.max_transmission_unit = self.mtu;
        caps.checksum.ipv4 = Checksum::Tx;
        caps.checksum.tcp = Checksum::Tx;
        caps.checksum.udp = Checksum::Tx;
        caps
    }
}

pub enum Cmd {
    OpenTcp {
        dst: SocketAddr,
        resp: OpenTcpResp,
    },
    OpenUdp {
        resp: OpenUdpResp,
    },
    SetAddrs {
        v4: Option<(Ipv4Addr, u8)>,
        v6: Option<(Ipv6Addr, u8)>,
    },
}

pub enum DataIn {
    Tcp(usize, Vec<u8>),
    TcpClose(usize),
    Udp(usize, SocketAddr, Vec<u8>),
    UdpClose(usize),
}

pub struct TcpConn {
    pub id: usize,
    pub from_stack: mpsc::Receiver<Vec<u8>>,
    data_in: mpsc::Sender<DataIn>,
    split: bool,
}

impl TcpConn {
    pub async fn send(&self, data: Vec<u8>) -> Result<()> {
        self.data_in
            .send(DataIn::Tcp(self.id, data))
            .await
            .map_err(|_| AetherError::Other("netstack closed".into()))
    }

    pub async fn close(&self) {
        let _ = self.data_in.send(DataIn::TcpClose(self.id)).await;
    }

    pub fn into_split(mut self) -> (TcpSender, mpsc::Receiver<Vec<u8>>) {
        self.split = true;
        (
            TcpSender {
                id: self.id,
                data_in: self.data_in.clone(),
            },
            std::mem::replace(&mut self.from_stack, {
                let (_tx, rx) = mpsc::channel(1);
                rx
            }),
        )
    }
}

impl Drop for TcpConn {
    fn drop(&mut self) {
        if !self.split {
            let _ = self.data_in.try_send(DataIn::TcpClose(self.id));
        }
    }
}

pub struct TcpSender {
    id: usize,
    data_in: mpsc::Sender<DataIn>,
}

impl TcpSender {
    pub async fn send(&self, data: Vec<u8>) -> Result<()> {
        self.data_in
            .send(DataIn::Tcp(self.id, data))
            .await
            .map_err(|_| AetherError::Other("netstack closed".into()))
    }

    pub async fn close(&self) {
        let _ = self.data_in.send(DataIn::TcpClose(self.id)).await;
    }
}

impl Drop for TcpSender {
    fn drop(&mut self) {
        let _ = self.data_in.try_send(DataIn::TcpClose(self.id));
    }
}

pub struct UdpConn {
    pub id: usize,
    pub from_stack: mpsc::Receiver<(SocketAddr, Vec<u8>)>,
    data_in: mpsc::Sender<DataIn>,
    split: bool,
}

impl UdpConn {
    pub async fn send_to(&self, dst: SocketAddr, data: Vec<u8>) -> Result<()> {
        self.data_in
            .send(DataIn::Udp(self.id, dst, data))
            .await
            .map_err(|_| AetherError::Other("netstack closed".into()))
    }

    pub async fn close(&self) {
        let _ = self.data_in.send(DataIn::UdpClose(self.id)).await;
    }

    pub fn into_split(mut self) -> (UdpSender, mpsc::Receiver<(SocketAddr, Vec<u8>)>) {
        self.split = true;
        (
            UdpSender {
                id: self.id,
                data_in: self.data_in.clone(),
            },
            std::mem::replace(&mut self.from_stack, {
                let (_tx, rx) = mpsc::channel(1);
                rx
            }),
        )
    }
}

impl Drop for UdpConn {
    fn drop(&mut self) {
        if !self.split {
            let _ = self.data_in.try_send(DataIn::UdpClose(self.id));
        }
    }
}

pub struct UdpSender {
    id: usize,
    data_in: mpsc::Sender<DataIn>,
}

impl UdpSender {
    pub async fn send_to(&self, dst: SocketAddr, data: Vec<u8>) -> Result<()> {
        self.data_in
            .send(DataIn::Udp(self.id, dst, data))
            .await
            .map_err(|_| AetherError::Other("netstack closed".into()))
    }

    pub async fn close(&self) {
        let _ = self.data_in.send(DataIn::UdpClose(self.id)).await;
    }
}

impl Drop for UdpSender {
    fn drop(&mut self) {
        let _ = self.data_in.try_send(DataIn::UdpClose(self.id));
    }
}

#[derive(Clone)]
pub struct StackHandle {
    cmd_tx: mpsc::Sender<Cmd>,
}

impl StackHandle {
    pub async fn open_tcp(&self, dst: SocketAddr) -> Result<TcpConn> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(Cmd::OpenTcp { dst, resp: resp_tx })
            .await
            .map_err(|_| AetherError::Other("netstack closed".into()))?;
        resp_rx
            .await
            .map_err(|_| AetherError::Other("netstack dropped".into()))?
            .map_err(AetherError::Other)
    }

    pub async fn open_udp(&self) -> Result<UdpConn> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(Cmd::OpenUdp { resp: resp_tx })
            .await
            .map_err(|_| AetherError::Other("netstack closed".into()))?;
        resp_rx
            .await
            .map_err(|_| AetherError::Other("netstack dropped".into()))?
            .map_err(AetherError::Other)
    }

    pub async fn set_addrs(
        &self,
        v4: Option<(Ipv4Addr, u8)>,
        v6: Option<(Ipv6Addr, u8)>,
    ) -> Result<()> {
        self.cmd_tx
            .send(Cmd::SetAddrs { v4, v6 })
            .await
            .map_err(|_| AetherError::Other("netstack closed".into()))
    }
}

struct TcpState {
    handle: SocketHandle,
    to_app: mpsc::Sender<Vec<u8>>,
    from_stack_rx: Option<mpsc::Receiver<Vec<u8>>>,
    connect_resp: Option<OpenTcpResp>,
    connect_deadline: std::time::Instant,
    pending: Vec<u8>,
    established: bool,
    half_closed: bool,
    /// 1.2.3-p1: this flow's own ordered backlog, drained into `pending` as the
    /// socket accepts bytes. Before this existed there was one GLOBAL deferred
    /// queue and a full `pending` on any single flow stopped the stack reading
    /// the shared `data_in` channel at all - so one stalled connection froze
    /// every other flow in the tunnel. A TCP stream may not lose or reorder a
    /// byte, so this is a queue and never a drop.
    overflow: VecDeque<Vec<u8>>,
    overflow_bytes: usize,
    orphaned_at: Option<std::time::Instant>,
    aborted: bool,
}

struct UdpState {
    handle: SocketHandle,
    to_app: mpsc::Sender<(SocketAddr, Vec<u8>)>,
}

pub struct NetStack {
    iface: Interface,
    device: StackDevice,
    sockets: SocketSet<'static>,
    tcp_conns: HashMap<usize, TcpState>,
    udp_conns: HashMap<usize, UdpState>,
    next_id: usize,
    next_port: u16,
    data_in_tx: mpsc::Sender<DataIn>,
    tcp_limits: TcpLimits,
}

fn strip_cidr(s: &str) -> &str {
    match s.split_once('/') {
        Some((ip, _)) => ip,
        None => s,
    }
}

fn to_ip_address(ip: IpAddr) -> IpAddress {
    match ip {
        IpAddr::V4(v4) => IpAddress::Ipv4(Ipv4Address::from(v4)),
        IpAddr::V6(v6) => IpAddress::Ipv6(Ipv6Address::from(v6)),
    }
}

fn to_ip_endpoint(addr: SocketAddr) -> IpEndpoint {
    IpEndpoint::new(to_ip_address(addr.ip()), addr.port())
}

fn cidr_prefix(s: &str) -> Option<u8> {
    s.split_once('/').and_then(|(_, p)| p.parse().ok())
}

fn parse_v4(s: &str) -> Result<Option<(Ipv4Addr, u8)>> {
    if s.is_empty() {
        return Ok(None);
    }
    let ip: Ipv4Addr = strip_cidr(s)
        .parse()
        .map_err(|_| AetherError::Other(format!("bad ipv4 {s}")))?;
    Ok(Some((ip, cidr_prefix(s).unwrap_or(32))))
}

fn parse_v6(s: &str) -> Result<Option<(Ipv6Addr, u8)>> {
    if s.is_empty() {
        return Ok(None);
    }
    let ip: Ipv6Addr = strip_cidr(s)
        .parse()
        .map_err(|_| AetherError::Other(format!("bad ipv6 {s}")))?;
    Ok(Some((ip, cidr_prefix(s).unwrap_or(128))))
}

fn routable_prefix_v4(p: u8) -> u8 {
    if p >= 31 {
        24
    } else {
        p
    }
}

fn routable_prefix_v6(p: u8) -> u8 {
    if p >= 127 {
        64
    } else {
        p
    }
}

fn apply_addrs(iface: &mut Interface, v4: Option<(Ipv4Addr, u8)>, v6: Option<(Ipv6Addr, u8)>) {
    iface.update_ip_addrs(|addrs| {
        addrs.clear();
        if let Some((ip, p)) = v4 {
            let _ = addrs.push(IpCidr::new(
                IpAddress::Ipv4(Ipv4Address::from(ip)),
                routable_prefix_v4(p),
            ));
        }
        if let Some((ip, p)) = v6 {
            let _ = addrs.push(IpCidr::new(
                IpAddress::Ipv6(Ipv6Address::from(ip)),
                routable_prefix_v6(p),
            ));
        }
    });

    if let Some((ip, _)) = v4 {
        let o = ip.octets();
        let host = if o[3] == 1 { 2 } else { 1 };
        let gw = Ipv4Address::new(o[0], o[1], o[2], host);
        let _ = iface.routes_mut().add_default_ipv4_route(gw);
    }
    if let Some((ip, _)) = v6 {
        let mut o = ip.octets();
        o[15] = if o[15] == 1 { 2 } else { 1 };
        let _ = iface
            .routes_mut()
            .add_default_ipv6_route(Ipv6Address::from(o));
    }
}

type AddrPair = (Option<(Ipv4Addr, u8)>, Option<(Ipv6Addr, u8)>);

fn current_addrs(iface: &Interface) -> AddrPair {
    let mut v4 = None;
    let mut v6 = None;
    for cidr in iface.ip_addrs() {
        match cidr {
            IpCidr::Ipv4(c) => v4 = Some((c.address(), c.prefix_len())),
            IpCidr::Ipv6(c) => v6 = Some((c.address(), c.prefix_len())),
        }
    }
    (v4, v6)
}

fn endpoint_to_socketaddr(ep: IpEndpoint) -> SocketAddr {
    let ip = match ep.addr {
        IpAddress::Ipv4(v4) => IpAddr::V4(v4.into()),
        IpAddress::Ipv6(v6) => IpAddr::V6(v6.into()),
    };
    SocketAddr::new(ip, ep.port)
}

pub fn spawn(
    ipv4: &str,
    ipv6: &str,
    mtu: usize,
    inbound_rx: mpsc::Receiver<Vec<u8>>,
    outbound_tx: mpsc::Sender<Vec<u8>>,
) -> Result<StackHandle> {
    spawn_with_limits(
        ipv4,
        ipv6,
        mtu,
        inbound_rx,
        outbound_tx,
        TcpLimits::from_env(),
    )
}

fn spawn_with_limits(
    ipv4: &str,
    ipv6: &str,
    mtu: usize,
    inbound_rx: mpsc::Receiver<Vec<u8>>,
    outbound_tx: mpsc::Sender<Vec<u8>>,
    tcp_limits: TcpLimits,
) -> Result<StackHandle> {
    let mut device = StackDevice::new(mtu);

    let config = Config::new(HardwareAddress::Ip);
    let mut iface = Interface::new(config, &mut device, Instant::now());

    let v4 = parse_v4(ipv4)?;
    let v6 = parse_v6(ipv6)?;
    apply_addrs(&mut iface, v4, v6);

    let (cmd_tx, cmd_rx) = mpsc::channel(256);
    let (data_in_tx, data_in_rx) = mpsc::channel(app_queue());

    let stack = NetStack {
        iface,
        device,
        sockets: SocketSet::new(Vec::new()),
        tcp_conns: HashMap::new(),
        udp_conns: HashMap::new(),
        next_id: 1,
        next_port: 49152,
        data_in_tx: data_in_tx.clone(),
        tcp_limits,
    };

    tokio::spawn(run(stack, cmd_rx, data_in_rx, inbound_rx, outbound_tx));

    Ok(StackHandle { cmd_tx })
}

fn alloc_port(p: &mut u16) -> u16 {
    let port = *p;
    *p = if port >= 65000 { 49152 } else { port + 1 };
    port
}

async fn run(
    mut s: NetStack,
    mut cmd_rx: mpsc::Receiver<Cmd>,
    mut data_in_rx: mpsc::Receiver<DataIn>,
    mut inbound_rx: mpsc::Receiver<Vec<u8>>,
    outbound_tx: mpsc::Sender<Vec<u8>>,
) -> Result<()> {
    let mut deferred: VecDeque<DataIn> = VecDeque::new();
    let mut tx_dropped: usize = 0;
    let mut next_drop_report: usize = DROP_REPORT_STEP;

    loop {
        let now = Instant::now();
        let poll_outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            s.iface.poll(now, &mut s.device, &mut s.sockets);
        }));
        if poll_outcome.is_err() {
            s.device.rx.clear();
            s.device.tx.clear();
        }
        // 1.2.3-p1 STARVATION FIX. The `select!` below is `biased` with the
        // inbound (download) arm first, so a saturated download used to take
        // every wake-up and neither of these queues ran until it paused: no new
        // flow could be opened and nothing could be written while a download was
        // in progress. Both get an explicit budget on every pass now.
        let mut n = 0;
        while n < MAX_CMD_PER_TICK {
            match cmd_rx.try_recv() {
                Ok(cmd) => {
                    handle_cmd(&mut s, cmd);
                    n += 1;
                }
                Err(_) => break,
            }
        }
        if deferred.is_empty() {
            let mut n = 0;
            while n < MAX_APP_INGEST_PER_TICK {
                match data_in_rx.try_recv() {
                    Ok(d) => {
                        n += 1;
                        if let Some(back) = try_handle_data(&mut s, d) {
                            deferred.push_back(back);
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }

        let tcp_busy = service_tcp(&mut s);
        let udp_busy = service_udp(&mut s);
        let dropped = flush_tx(&mut s, &outbound_tx);
        // Retained packets (see [flush_tx]) must be retried soon, not on the
        // next idle tick, or holding them instead of dropping them would just be
        // a slower way of delaying them.
        let tx_retained = !s.device.tx.is_empty();

        if dropped > 0 {
            tx_dropped = tx_dropped.saturating_add(dropped);
            if tx_dropped >= next_drop_report {
                next_drop_report = tx_dropped + DROP_REPORT_STEP;
                log::debug!(
                    "[netstack] dropped {tx_dropped} outbound packets past the {MAX_TX_RETAINED}-packet retain window"
                );
            }
        }

        while let Some(d) = deferred.pop_front() {
            if let Some(back) = try_handle_data(&mut s, d) {
                deferred.push_front(back);
                break;
            }
        }

        let delay = if tcp_busy || udp_busy || tx_retained || !deferred.is_empty() {
            Some(BACKPRESSURE_RETRY)
        } else {
            let polled = s
                .iface
                .poll_delay(Instant::now(), &s.sockets)
                .map(|d| std::time::Duration::from_micros(d.total_micros()));

            if s.tcp_conns.is_empty() && s.udp_conns.is_empty() {
                polled
            } else {
                Some(polled.map_or(MAX_IDLE_TICK, |d| d.min(MAX_IDLE_TICK)))
            }
        };

        tokio::select! {
            biased;

            maybe = inbound_rx.recv() => {
                match maybe {
                    Some(pkt) => {
                        s.device.rx.push_back(pkt);
                        let mut n = 0;
                        while n < MAX_INGEST_PER_TICK {
                            match inbound_rx.try_recv() {
                                Ok(p) => { s.device.rx.push_back(p); n += 1; }
                                Err(_) => break,
                            }
                        }
                    }
                    None => return Ok(()),
                }
            }

            maybe = cmd_rx.recv() => {
                match maybe {
                    Some(cmd) => handle_cmd(&mut s, cmd),
                    None => return Ok(()),
                }
            }

            maybe = data_in_rx.recv(), if deferred.is_empty() => {
                if let Some(d) = maybe {
                    if let Some(back) = try_handle_data(&mut s, d) {
                        deferred.push_back(back);
                    } else {
                        while deferred.is_empty() {
                            match data_in_rx.try_recv() {
                                Ok(d2) => {
                                    if let Some(back) = try_handle_data(&mut s, d2) {
                                        deferred.push_back(back);
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }
            }

            _ = sleep_opt(delay) => {}
        }
    }
}

async fn sleep_opt(delay: Option<std::time::Duration>) {
    match delay {
        Some(d) => tokio::time::sleep(d).await,
        None => std::future::pending::<()>().await,
    }
}

fn handle_cmd(s: &mut NetStack, cmd: Cmd) {
    match cmd {
        Cmd::OpenTcp { dst, resp } => {
            let rx_buf = tcp::SocketBuffer::new(vec![0u8; tcp_rx_buf()]);
            let tx_buf = tcp::SocketBuffer::new(vec![0u8; tcp_tx_buf()]);
            let mut socket = tcp::Socket::new(rx_buf, tx_buf);
            socket.set_nagle_enabled(false);
            let keepalive = s.tcp_limits.keepalive;
            socket.set_keep_alive(Some(smol_duration(keepalive)));
            socket.set_timeout(Some(smol_duration(keepalive.saturating_mul(3))));

            // ==============================================================
            // 1.2.3-p1: give this sender a congestion window.
            //
            // Until this line existed every flow leaving this PC through the
            // tunnel ran on smoltcp's `NoControl` controller, because
            // `Cargo.toml` set `default-features = false` and never re-enabled
            // `socket-tcp-cubic`. NoControl is not a conservative controller, it
            // is the ABSENCE of one: no slow start, no congestion window, no
            // reduction on loss. The sender writes as fast as the peer's window
            // allows and answers congestion by retransmitting harder.
            //
            // Aether TERMINATES TCP here, so this stack - not Windows' own TCP -
            // owns congestion control for everything the machine sends through
            // the tunnel. An unthrottled sender builds a standing queue on the
            // uplink, and the ACKs of the DOWNLOAD direction sit in that same
            // queue: that is how an upload problem shows up as "the download is
            // slow". In chained `Aether -> Psiphon` mode the whole PC rides one
            // SSH connection, so that queue is in front of every flow at once.
            //
            // CUBIC rather than Reno: this is a high-RTT, lossy, two-hop path and
            // CUBIC's window growth is RTT-independent, which is exactly the
            // regime Reno handles worst. The variant only exists when
            // `socket-tcp-cubic` is enabled, so a future edit that drops the
            // feature FAILS THE BUILD instead of quietly shipping this again.
            // ==============================================================
            // >>> AETHER-APP-PATCH netstack-congestion-control
            // ویژگی socket-tcp-cubic در Cargo.toml جفتِ همین خط است؛
            // بی آن، AnyController::new() به NoControl می‌رسد.
            socket.set_congestion_control(tcp::CongestionControl::Cubic);
            // <<< AETHER-APP-PATCH netstack-congestion-control
            // Without these a flow whose peer vanishes mid-transfer stays
            // Established forever, retransmitting into nothing and pinning its
            // buffers for the rest of the session.
            socket.set_keep_alive(Some(TCP_KEEPALIVE));
            socket.set_timeout(Some(TCP_DEAD_PEER_TIMEOUT));

            let local_port = alloc_port(&mut s.next_port);
            let remote = to_ip_endpoint(dst);

            if let Err(e) = socket.connect(s.iface.context(), remote, local_port) {
                let _ = resp.send(Err(format!("connect: {e:?}")));
                return;
            }

            let handle = s.sockets.add(socket);
            let id = s.next_id;
            s.next_id += 1;

            let (to_app_tx, to_app_rx) = mpsc::channel(app_queue());

            s.tcp_conns.insert(
                id,
                TcpState {
                    handle,
                    to_app: to_app_tx,
                    from_stack_rx: Some(to_app_rx),
                    connect_resp: Some(resp),
                    connect_deadline: std::time::Instant::now() + s.tcp_limits.connect,
                    pending: Vec::new(),
                    established: false,
                    half_closed: false,
                    overflow: VecDeque::new(),
                    overflow_bytes: 0,
                    orphaned_at: None,
                    aborted: false,
                },
            );
        }
        Cmd::OpenUdp { resp } => {
            let rx_meta = vec![udp::PacketMetadata::EMPTY; udp_meta()];
            let tx_meta = vec![udp::PacketMetadata::EMPTY; udp_meta()];
            let rx_buf = udp::PacketBuffer::new(rx_meta, vec![0u8; udp_buf()]);
            let tx_buf = udp::PacketBuffer::new(tx_meta, vec![0u8; udp_buf()]);
            let mut socket = udp::Socket::new(rx_buf, tx_buf);

            let local_port = alloc_port(&mut s.next_port);
            if let Err(e) = socket.bind(local_port) {
                let _ = resp.send(Err(format!("bind: {e:?}")));
                return;
            }

            let handle = s.sockets.add(socket);
            let id = s.next_id;
            s.next_id += 1;

            let (to_app_tx, to_app_rx) = mpsc::channel(app_queue());
            s.udp_conns.insert(
                id,
                UdpState {
                    handle,
                    to_app: to_app_tx,
                },
            );

            let conn = UdpConn {
                id,
                from_stack: to_app_rx,
                data_in: s.data_in_tx.clone(),
                split: false,
            };
            let _ = resp.send(Ok(conn));
        }
        Cmd::SetAddrs { v4, v6 } => {
            let (current_v4, current_v6) = current_addrs(&s.iface);
            apply_addrs(&mut s.iface, v4.or(current_v4), v6.or(current_v6));
            log::info!("netstack addresses synchronized from edge capsule");
        }
    }
}

/// Returns `Some(d)` when the datagram must be deferred on the SHARED channel.
///
/// 1.2.3-p1: that now happens only when a single flow has already queued
/// [`MAX_FLOW_BACKLOG_BYTES`] of its own, i.e. when deferring is genuine memory
/// backpressure rather than "this one socket is momentarily full". A full
/// `pending` used to be enough to stop the stack reading `data_in` at all, which
/// froze every other flow in the tunnel behind one slow connection.
fn try_handle_data(s: &mut NetStack, d: DataIn) -> Option<DataIn> {
    match d {
        DataIn::Tcp(id, data) => {
            if let Some(st) = s.tcp_conns.get_mut(&id) {
                let max = max_tcp_pending();
                // Anything already queued for this flow must stay in front of
                // `data`: a TCP stream may not be reordered.
                if !st.overflow.is_empty() || st.pending.len() >= max {
                    if st.overflow_bytes >= MAX_FLOW_BACKLOG_BYTES {
                        // This flow, and only this flow, pushes back.
                        return Some(DataIn::Tcp(id, data));
                    }
                    st.overflow_bytes += data.len();
                    st.overflow.push_back(data);
                    return None;
                }
                let space = max - st.pending.len();
                if data.len() <= space {
                    st.pending.extend_from_slice(&data);
                } else {
                    st.pending.extend_from_slice(&data[..space]);
                    let rest = data[space..].to_vec();
                    st.overflow_bytes += rest.len();
                    st.overflow.push_back(rest);
                }
            }
            None
        }
        DataIn::TcpClose(id) => {
            if let Some(st) = s.tcp_conns.get_mut(&id) {
                st.half_closed = true;
            }
            None
        }
        DataIn::Udp(id, dst, data) => {
            if let Some(st) = s.udp_conns.get(&id) {
                let sock = s.sockets.get_mut::<udp::Socket>(st.handle);
                let _ = sock.send_slice(&data, to_ip_endpoint(dst));
            }
            None
        }
        DataIn::UdpClose(id) => {
            if let Some(st) = s.udp_conns.remove(&id) {
                s.sockets.remove(st.handle);
            }
            None
        }
    }
}

fn service_tcp(s: &mut NetStack) -> bool {
    let mut backpressured = false;
    let ids: Vec<usize> = s.tcp_conns.keys().copied().collect();
    let now = std::time::Instant::now();

    for id in ids {
        let handle = match s.tcp_conns.get(&id) {
            Some(st) => st.handle,
            None => continue,
        };

        if s.tcp_conns[&id].aborted {
            s.sockets.remove(handle);
            s.tcp_conns.remove(&id);
            continue;
        }

        let state = s.sockets.get_mut::<tcp::Socket>(handle).state();
        let data_in_tx = s.data_in_tx.clone();

        let connected = matches!(state, tcp::State::Established | tcp::State::CloseWait);
        if !s.tcp_conns[&id].established && connected {
            if let Some(st) = s.tcp_conns.get_mut(&id) {
                st.established = true;
                if let (Some(resp), Some(rx)) = (st.connect_resp.take(), st.from_stack_rx.take()) {
                    let conn = TcpConn {
                        id,
                        from_stack: rx,
                        data_in: data_in_tx.clone(),
                        split: false,
                    };
                    let _ = resp.send(Ok(conn));
                }
            }
        }

        if !s.tcp_conns[&id].established
            && matches!(state, tcp::State::Closed | tcp::State::TimeWait)
        {
            if let Some(st) = s.tcp_conns.get_mut(&id) {
                if let Some(resp) = st.connect_resp.take() {
                    let _ = resp.send(Err("connection refused".into()));
                }
            }
            s.sockets.remove(handle);
            s.tcp_conns.remove(&id);
            continue;
        }

        if !s.tcp_conns[&id].established {
            let st = s.tcp_conns.get_mut(&id).unwrap();
            let abandoned = st.connect_resp.as_ref().is_none_or(|resp| resp.is_closed());
            if abandoned || now >= st.connect_deadline {
                if let Some(resp) = st.connect_resp.take() {
                    let _ = resp.send(Err("connection timed out".into()));
                }
                s.sockets.remove(handle);
                s.tcp_conns.remove(&id);
            }
            continue;
        }

        {
            let socket = s.sockets.get_mut::<tcp::Socket>(handle);
            if socket.can_send() {
                let st = s.tcp_conns.get_mut(&id).unwrap();
                if !st.pending.is_empty() {
                    let sent = socket.send_slice(&st.pending).unwrap_or(0);
                    if sent > 0 {
                        st.pending.drain(0..sent);
                        if st.pending.len() * 4 < st.pending.capacity() {
                            st.pending
                                .shrink_to(max_tcp_pending().min(st.pending.capacity()));
                        }
                    }
                }
                // 1.2.3-p1: refill from this flow's own backlog, in order. This
                // is what makes the backlog per-flow rather than a shared queue
                // that any one connection could freeze.
                let max = max_tcp_pending();
                while st.pending.len() < max {
                    let Some(chunk) = st.overflow.pop_front() else {
                        break;
                    };
                    let space = max - st.pending.len();
                    if chunk.len() <= space {
                        st.overflow_bytes = st.overflow_bytes.saturating_sub(chunk.len());
                        st.pending.extend_from_slice(&chunk);
                    } else {
                        st.pending.extend_from_slice(&chunk[..space]);
                        st.overflow_bytes = st.overflow_bytes.saturating_sub(space);
                        st.overflow.push_front(chunk[space..].to_vec());
                        break;
                    }
                }
                if !st.overflow.is_empty() {
                    backpressured = true;
                }
            }
        }

        {
            let pending_empty =
                s.tcp_conns[&id].pending.is_empty() && s.tcp_conns[&id].overflow.is_empty();
            let half = s.tcp_conns[&id].half_closed;
            if half && pending_empty {
                s.sockets.get_mut::<tcp::Socket>(handle).close();
            }
        }

        let to_app = s.tcp_conns[&id].to_app.clone();
        let mut app_gone = false;
        let mut delivered = 0;

        while delivered < MAX_RECV_CHUNKS {
            let permit = match to_app.try_reserve() {
                Ok(permit) => permit,
                Err(mpsc::error::TrySendError::Full(())) => {
                    backpressured = true;
                    break;
                }
                Err(mpsc::error::TrySendError::Closed(())) => {
                    app_gone = true;
                    break;
                }
            };

            let socket = s.sockets.get_mut::<tcp::Socket>(handle);
            if !socket.can_recv() {
                break;
            }
            let chunk = match socket.recv(|buf| {
                let v = buf.to_vec();
                (v.len(), v)
            }) {
                Ok(v) if !v.is_empty() => v,
                _ => break,
            };
            permit.send(chunk);
            delivered += 1;
        }

        if app_gone {
            let st = s.tcp_conns.get_mut(&id).unwrap();
            let socket = s.sockets.get_mut::<tcp::Socket>(handle);
            let orphaned_at = *st.orphaned_at.get_or_insert(now);
            if socket.can_recv() || now.duration_since(orphaned_at) >= s.tcp_limits.orphan_linger {
                if socket.state() != tcp::State::Closed {
                    socket.abort();
                }
                st.aborted = true;
                continue;
            }
            socket.close();
        }

        let st_state = s.sockets.get_mut::<tcp::Socket>(handle).state();
        if matches!(st_state, tcp::State::CloseWait) {
            s.sockets.get_mut::<tcp::Socket>(handle).close();
        }
        if matches!(st_state, tcp::State::TimeWait) {
            if let Some(st) = s.tcp_conns.get_mut(&id) {
                st.pending.clear();
                st.pending.shrink_to_fit();
                st.overflow.clear();
                st.overflow_bytes = 0;
            }
        }
        if matches!(st_state, tcp::State::Closed | tcp::State::TimeWait)
            && s.tcp_conns[&id].established
        {
            s.sockets.remove(handle);
            s.tcp_conns.remove(&id);
        }
    }

    backpressured
}

fn service_udp(s: &mut NetStack) -> bool {
    let mut backpressured = false;
    let ids: Vec<usize> = s.udp_conns.keys().copied().collect();

    for id in ids {
        let handle = match s.udp_conns.get(&id) {
            Some(st) => st.handle,
            None => continue,
        };

        let to_app = s.udp_conns[&id].to_app.clone();
        let mut delivered = 0;
        let mut app_gone = false;

        while delivered < MAX_RECV_CHUNKS {
            let permit = match to_app.try_reserve() {
                Ok(permit) => permit,
                Err(mpsc::error::TrySendError::Full(())) => {
                    backpressured = true;
                    break;
                }
                Err(mpsc::error::TrySendError::Closed(())) => {
                    app_gone = true;
                    break;
                }
            };

            let socket = s.sockets.get_mut::<udp::Socket>(handle);
            if !socket.can_recv() {
                break;
            }
            match socket.recv() {
                Ok((data, meta)) => {
                    permit.send((endpoint_to_socketaddr(meta.endpoint), data.to_vec()));
                    delivered += 1;
                }
                Err(_) => break,
            }
        }

        if app_gone {
            if let Some(st) = s.udp_conns.remove(&id) {
                s.sockets.remove(st.handle);
            }
        }
    }

    backpressured
}

/// Hands the device transmit ring to the WireGuard writer.
///
/// 1.2.3-p1: this used to TAIL-DROP everything that did not fit the moment the
/// writer was momentarily behind. During a download the majority of what does
/// not fit is an ACK, and silently discarding the ACKs of the flow you are
/// trying to speed up makes the REMOTE sender halve its window for no reason -
/// loss that no counter in this process could see and that the congestion
/// controller on the far side reads as a congested path. The burst is retained
/// across a couple of [`BACKPRESSURE_RETRY`] passes now, and only what is past
/// [`MAX_TX_RETAINED`] is dropped, which is what a real link does when its queue
/// is genuinely full.
fn flush_tx(s: &mut NetStack, outbound_tx: &mpsc::Sender<Vec<u8>>) -> usize {
    let mut dropped = 0;
    while let Some(pkt) = s.device.tx.pop_front() {
        match outbound_tx.try_send(pkt) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(pkt)) => {
                s.device.tx.push_front(pkt);
                // Bounded, so retaining can never become seconds of hidden queue.
                while s.device.tx.len() > MAX_TX_RETAINED {
                    s.device.tx.pop_back();
                    dropped += 1;
                }
                break;
            }
            Err(mpsc::error::TrySendError::Closed(_)) => break,
        }
    }
    dropped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    fn udp_ip_packet(payload_len: usize) -> Vec<u8> {
        let total = 20 + 8 + payload_len;
        let mut pkt = vec![0u8; total];
        pkt[0] = 0x45;
        pkt[2] = (total >> 8) as u8;
        pkt[3] = (total & 0xff) as u8;
        pkt[8] = 64;
        pkt[9] = 17;
        pkt[12..16].copy_from_slice(&[10, 0, 0, 9]);
        pkt[16..20].copy_from_slice(&[198, 18, 0, 1]);
        pkt[20..22].copy_from_slice(&5555u16.to_be_bytes());
        pkt[22..24].copy_from_slice(&9999u16.to_be_bytes());
        let udp_len = (8 + payload_len) as u16;
        pkt[24..26].copy_from_slice(&udp_len.to_be_bytes());
        pkt
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn netstack_keeps_draining_inbound_when_outbound_is_never_read() {
        let (inbound_tx, inbound_rx) = mpsc::channel::<Vec<u8>>(4);
        let (outbound_tx, _outbound_rx_never_read) = mpsc::channel::<Vec<u8>>(1);

        let stack = spawn("198.18.0.1", "fc00::1", 1400, inbound_rx, outbound_tx)
            .expect("netstack should start");

        let udp = stack.open_udp().await.expect("udp socket should open");
        let dst: SocketAddr = "1.1.1.1:53".parse().unwrap();

        for _ in 0..64 {
            let _ = udp.send_to(dst, vec![0u8; 64]).await;
        }

        tokio::time::sleep(StdDuration::from_millis(120)).await;

        for index in 0..64 {
            let send = inbound_tx.send(udp_ip_packet(32));
            tokio::time::timeout(StdDuration::from_secs(3), send)
                .await
                .unwrap_or_else(|_| {
                    panic!("netstack stopped draining inbound at packet {index}: deadlock")
                })
                .expect("inbound channel should stay open");
        }
    }

    fn checksum16(data: &[u8], initial: u32) -> u16 {
        let mut sum = initial;
        let mut chunks = data.chunks_exact(2);
        for chunk in chunks.by_ref() {
            sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
        }
        if let Some(&last) = chunks.remainder().first() {
            sum += (last as u32) << 8;
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        !(sum as u16)
    }

    struct Segment {
        src_port: u16,
        dst_port: u16,
        seq: u32,
        flags: u8,
    }

    fn parse_tcp(pkt: &[u8]) -> Option<Segment> {
        if pkt.len() < 20 || pkt[0] >> 4 != 4 {
            return None;
        }
        let ihl = ((pkt[0] & 0x0f) as usize) * 4;
        if pkt[9] != 6 || pkt.len() < ihl + 20 {
            return None;
        }
        let tcp = &pkt[ihl..];
        Some(Segment {
            src_port: u16::from_be_bytes([tcp[0], tcp[1]]),
            dst_port: u16::from_be_bytes([tcp[2], tcp[3]]),
            seq: u32::from_be_bytes([tcp[4], tcp[5], tcp[6], tcp[7]]),
            flags: tcp[13],
        })
    }

    fn build_tcp(
        src: (Ipv4Addr, u16),
        dst: (Ipv4Addr, u16),
        seq: u32,
        ack: u32,
        flags: u8,
    ) -> Vec<u8> {
        let mut tcp = vec![0u8; 20];
        tcp[0..2].copy_from_slice(&src.1.to_be_bytes());
        tcp[2..4].copy_from_slice(&dst.1.to_be_bytes());
        tcp[4..8].copy_from_slice(&seq.to_be_bytes());
        tcp[8..12].copy_from_slice(&ack.to_be_bytes());
        tcp[12] = 5 << 4;
        tcp[13] = flags;
        tcp[14..16].copy_from_slice(&64240u16.to_be_bytes());

        let mut pseudo = Vec::new();
        pseudo.extend_from_slice(&src.0.octets());
        pseudo.extend_from_slice(&dst.0.octets());
        pseudo.push(0);
        pseudo.push(6);
        pseudo.extend_from_slice(&(tcp.len() as u16).to_be_bytes());
        pseudo.extend_from_slice(&tcp);
        let tcp_sum = checksum16(&pseudo, 0);
        tcp[16..18].copy_from_slice(&tcp_sum.to_be_bytes());

        let total = 20 + tcp.len();
        let mut ip = vec![0u8; 20];
        ip[0] = 0x45;
        ip[2..4].copy_from_slice(&(total as u16).to_be_bytes());
        ip[8] = 64;
        ip[9] = 6;
        ip[12..16].copy_from_slice(&src.0.octets());
        ip[16..20].copy_from_slice(&dst.0.octets());
        let ip_sum = checksum16(&ip, 0);
        ip[10..12].copy_from_slice(&ip_sum.to_be_bytes());

        ip.extend_from_slice(&tcp);
        ip
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_vanished_app_makes_the_netstack_tear_the_connection_down() {
        let local = Ipv4Addr::new(198, 18, 0, 1);
        let remote = Ipv4Addr::new(93, 184, 216, 34);
        let remote_port = 80u16;

        let (inbound_tx, inbound_rx) = mpsc::channel::<Vec<u8>>(64);
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<Vec<u8>>(256);

        let stack = spawn("198.18.0.1", "fc00::1", 1400, inbound_rx, outbound_tx)
            .expect("netstack should start");

        let dst = SocketAddr::new(IpAddr::V4(remote), remote_port);
        let connect = {
            let stack = stack.clone();
            tokio::spawn(async move { stack.open_tcp(dst).await })
        };

        let deadline = tokio::time::Instant::now() + StdDuration::from_secs(5);

        let (client_port, client_seq) = loop {
            let pkt = tokio::time::timeout_at(deadline, outbound_rx.recv())
                .await
                .expect("the netstack should emit a syn")
                .expect("outbound channel stays open");

            if let Some(seg) = parse_tcp(&pkt) {
                if seg.dst_port == remote_port && seg.flags & 0x02 != 0 && seg.flags & 0x10 == 0 {
                    break (seg.src_port, seg.seq);
                }
            }
        };

        let syn_ack = build_tcp(
            (remote, remote_port),
            (local, client_port),
            5000,
            client_seq.wrapping_add(1),
            0x12,
        );
        inbound_tx
            .send(syn_ack)
            .await
            .expect("inbound accepts the syn-ack");

        let conn = tokio::time::timeout(StdDuration::from_secs(5), connect)
            .await
            .expect("the connect call should finish")
            .expect("the connect task should not panic")
            .expect("the connection should be established");

        drop(conn);

        let deadline = tokio::time::Instant::now() + StdDuration::from_secs(5);
        let mut saw_teardown = false;

        while let Ok(Some(pkt)) = tokio::time::timeout_at(deadline, outbound_rx.recv()).await {
            if let Some(seg) = parse_tcp(&pkt) {
                if seg.flags & 0x01 != 0 || seg.flags & 0x04 != 0 {
                    saw_teardown = true;
                    break;
                }
            }
        }

        assert!(
            saw_teardown,
            "the netstack never closed the socket after the app went away, so it leaks"
        );
    }

    fn quick_limits() -> TcpLimits {
        TcpLimits {
            connect: StdDuration::from_millis(300),
            keepalive: StdDuration::from_secs(60),
            orphan_linger: StdDuration::from_millis(300),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_connect_nobody_answers_fails_instead_of_hanging() {
        let (_inbound_tx, inbound_rx) = mpsc::channel::<Vec<u8>>(64);
        let (outbound_tx, _outbound_rx) = mpsc::channel::<Vec<u8>>(256);
        let stack = spawn_with_limits(
            "198.18.0.1",
            "fc00::1",
            1400,
            inbound_rx,
            outbound_tx,
            quick_limits(),
        )
        .expect("netstack should start");

        let dst: SocketAddr = "93.184.216.34:80".parse().unwrap();
        let outcome = tokio::time::timeout(StdDuration::from_secs(5), stack.open_tcp(dst))
            .await
            .expect("a connect that is never answered must fail, not hang");

        match outcome {
            Ok(_) => panic!("nothing answered, so the connect cannot succeed"),
            Err(error) => assert!(error.to_string().contains("timed out"), "{error}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_orphan_whose_far_end_never_closes_is_reset() {
        let local = Ipv4Addr::new(198, 18, 0, 1);
        let remote = Ipv4Addr::new(93, 184, 216, 34);
        let remote_port = 80u16;

        let (inbound_tx, inbound_rx) = mpsc::channel::<Vec<u8>>(64);
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<Vec<u8>>(256);
        let stack = spawn_with_limits(
            "198.18.0.1",
            "fc00::1",
            1400,
            inbound_rx,
            outbound_tx,
            quick_limits(),
        )
        .expect("netstack should start");

        let dst = SocketAddr::new(IpAddr::V4(remote), remote_port);
        let connect = {
            let stack = stack.clone();
            tokio::spawn(async move { stack.open_tcp(dst).await })
        };

        let deadline = tokio::time::Instant::now() + StdDuration::from_secs(5);
        let (client_port, client_seq) = loop {
            let pkt = tokio::time::timeout_at(deadline, outbound_rx.recv())
                .await
                .expect("the netstack should emit a syn")
                .expect("outbound channel stays open");
            if let Some(seg) = parse_tcp(&pkt) {
                if seg.dst_port == remote_port && seg.flags & 0x02 != 0 && seg.flags & 0x10 == 0 {
                    break (seg.src_port, seg.seq);
                }
            }
        };

        let syn_ack = build_tcp(
            (remote, remote_port),
            (local, client_port),
            5000,
            client_seq.wrapping_add(1),
            0x12,
        );
        inbound_tx
            .send(syn_ack)
            .await
            .expect("inbound accepts the syn-ack");

        let conn = tokio::time::timeout(StdDuration::from_secs(5), connect)
            .await
            .expect("the connect call should finish")
            .expect("the connect task should not panic")
            .expect("the connection should be established");
        drop(conn);

        let fin_seq = loop {
            let pkt = tokio::time::timeout_at(deadline, outbound_rx.recv())
                .await
                .expect("the netstack should send a fin")
                .expect("outbound channel stays open");
            if let Some(seg) = parse_tcp(&pkt) {
                if seg.flags & 0x01 != 0 {
                    break seg.seq;
                }
            }
        };
        let ack = build_tcp(
            (remote, remote_port),
            (local, client_port),
            5001,
            fin_seq.wrapping_add(1),
            0x10,
        );
        inbound_tx.send(ack).await.expect("inbound accepts the ack");

        let mut saw_reset = false;
        while let Ok(Some(pkt)) = tokio::time::timeout_at(deadline, outbound_rx.recv()).await {
            if let Some(seg) = parse_tcp(&pkt) {
                if seg.flags & 0x04 != 0 {
                    saw_reset = true;
                    break;
                }
            }
        }
        assert!(
            saw_reset,
            "an orphaned connection whose far end never closes must be reset, not kept"
        );
    }

    #[tokio::test]
    async fn an_address_assigned_for_one_family_keeps_the_other() {
        let mut device = StackDevice::new(1400);
        let mut iface = Interface::new(
            Config::new(HardwareAddress::Ip),
            &mut device,
            Instant::now(),
        );
        apply_addrs(
            &mut iface,
            Some(("172.16.0.2".parse().unwrap(), 32)),
            Some(("2606:4700:110:8a36::1".parse().unwrap(), 128)),
        );
        let mut stack = NetStack {
            iface,
            device,
            sockets: SocketSet::new(Vec::new()),
            tcp_conns: HashMap::new(),
            udp_conns: HashMap::new(),
            next_id: 0,
            next_port: 40000,
            data_in_tx: mpsc::channel(1).0,
            tcp_limits: TcpLimits::from_env(),
        };

        handle_cmd(
            &mut stack,
            Cmd::SetAddrs {
                v4: Some(("172.16.0.9".parse().unwrap(), 32)),
                v6: None,
            },
        );
        handle_cmd(
            &mut stack,
            Cmd::SetAddrs {
                v4: None,
                v6: Some(("2606:4700:110:8a36::9".parse().unwrap(), 128)),
            },
        );

        let (v4, v6) = current_addrs(&stack.iface);
        assert_eq!(v4.map(|(ip, _)| ip), Some("172.16.0.9".parse().unwrap()));
        assert_eq!(
            v6.map(|(ip, _)| ip),
            Some("2606:4700:110:8a36::9".parse().unwrap())
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn flush_tx_retains_the_burst_instead_of_shredding_it() {
        let (outbound_tx, outbound_rx) = mpsc::channel::<Vec<u8>>(2);
        let mut stack = NetStack {
            iface: {
                let mut device = StackDevice::new(1400);
                let config = Config::new(HardwareAddress::Ip);
                Interface::new(config, &mut device, Instant::now())
            },
            device: StackDevice::new(1400),
            sockets: SocketSet::new(Vec::new()),
            tcp_conns: HashMap::new(),
            udp_conns: HashMap::new(),
            next_id: 0,
            next_port: 40000,
            data_in_tx: mpsc::channel(1).0,
            tcp_limits: TcpLimits::from_env(),
        };

        for _ in 0..10 {
            stack.device.tx.push_back(vec![1, 2, 3]);
        }

        let dropped = flush_tx(&mut stack, &outbound_tx);

        // 1.2.3-p1: what does not fit is HELD for the next pass, not discarded.
        // During a download most of it is an ACK, and dropping those makes the
        // remote sender halve its window for no reason.
        assert_eq!(outbound_rx.len(), 2, "the channel keeps what fits");
        assert_eq!(
            dropped, 0,
            "a burst of 10 is well inside the retain window, so nothing may be dropped"
        );
        assert_eq!(
            stack.device.tx.len(),
            8,
            "the rest must still be queued for the next pass"
        );
        assert_eq!(
            outbound_tx.capacity(),
            0,
            "sanity: the channel really was full when the burst was retained"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn flush_tx_still_bounds_the_retain_window() {
        let (outbound_tx, _outbound_rx) = mpsc::channel::<Vec<u8>>(1);
        let mut stack = NetStack {
            iface: {
                let mut device = StackDevice::new(1400);
                let config = Config::new(HardwareAddress::Ip);
                Interface::new(config, &mut device, Instant::now())
            },
            device: StackDevice::new(1400),
            sockets: SocketSet::new(Vec::new()),
            tcp_conns: HashMap::new(),
            udp_conns: HashMap::new(),
            next_id: 0,
            next_port: 40000,
            data_in_tx: mpsc::channel(1).0,
            // >>> AETHER-APP-PATCH core-test-target-compiles
            // این فیلد در دو تستِ دیگر همین فایل (خطوط ۱۵۳۶ و ۱۵۷۸) مقدار
            // می‌گیرد و اینجا جا افتاده بود، پس `cargo test` روی کرِیت هسته با
            // E0063 رد می‌شد و **هیچ‌کدام** از تست‌های هسته اجرا نمی‌شدند. این
            // فقط cfg(test) است، پس باینریِ منتشرشده سالم بود — و همین توضیح
            // می‌دهد که چطور می‌شد «بیلد درست است» و باز خطا سر برآورد.
            tcp_limits: TcpLimits::from_env(),
            // <<< AETHER-APP-PATCH core-test-target-compiles
        };

        for _ in 0..(MAX_TX_RETAINED + 50) {
            stack.device.tx.push_back(vec![1, 2, 3]);
        }

        let dropped = flush_tx(&mut stack, &outbound_tx);

        assert_eq!(
            stack.device.tx.len(),
            MAX_TX_RETAINED,
            "retaining is bounded, so it can never become seconds of hidden queue"
        );
        assert_eq!(
            dropped, 49,
            "everything past the window is dropped, and counted"
        );
    }
}
