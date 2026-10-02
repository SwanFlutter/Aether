use std::net::SocketAddr;
use std::path::PathBuf;

use crate::error::{AetherError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Chain,
    Reverse,
    Only,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Chain => "tor through the tunnel",
            Mode::Reverse => "the tunnel through tor",
            Mode::Only => "tor alone",
        }
    }
}

pub fn mode() -> Mode {
    let raw = match std::env::var("AETHER_TOR") {
        Ok(value) => value.trim().to_lowercase(),
        Err(_) => return Mode::Off,
    };

    match raw.as_str() {
        "" | "0" | "off" | "false" | "no" => Mode::Off,
        "reverse" | "rev" | "warp-over-tor" => Mode::Reverse,
        "only" | "alone" | "tor-only" | "direct" => Mode::Only,
        _ => Mode::Chain,
    }
}

pub fn listen_address() -> SocketAddr {
    std::env::var("AETHER_TOR_BIND")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or_else(|| "127.0.0.1:1820".parse().expect("a literal address"))
}

pub fn state_dir(base_config: &str) -> PathBuf {
    if let Some(dir) = std::env::var("AETHER_TOR_DIR")
        .ok()
        .filter(|d| !d.trim().is_empty())
    {
        return PathBuf::from(dir.trim());
    }

    let config = PathBuf::from(base_config);
    let stem = config
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "aether".to_string());

    match config.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(format!("{stem}-tor")),
        _ => PathBuf::from(format!("{stem}-tor")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bridges {
    Disabled,
    Manual(Vec<String>),
    Auto { forced: bool },
}

pub fn bridges() -> Bridges {
    let raw = std::env::var("AETHER_TOR_BRIDGES").unwrap_or_default();
    let trimmed = raw.trim();

    match trimmed.to_lowercase().as_str() {
        "" => Bridges::Auto { forced: false },
        "off" | "no" | "0" | "false" | "none" => Bridges::Disabled,
        "auto" | "on" | "1" | "yes" | "true" | "force" => Bridges::Auto { forced: true },
        _ => {
            let lines: Vec<String> = trimmed
                .split(['\n', ';'])
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();

            if lines.is_empty() {
                Bridges::Auto { forced: false }
            } else {
                Bridges::Manual(lines)
            }
        }
    }
}

pub fn bridge_lines() -> Vec<String> {
    match bridges() {
        Bridges::Manual(lines) => lines,
        _ => Vec::new(),
    }
}

pub fn transports() -> Vec<(String, String)> {
    std::env::var("AETHER_TOR_PT")
        .unwrap_or_default()
        .split(['\n', ';'])
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            match entry.split_once('=') {
                Some((protocol, path)) => {
                    Some((protocol.trim().to_string(), path.trim().to_string()))
                }
                None => Some(("obfs4".to_string(), entry.to_string())),
            }
        })
        .collect()
}

#[cfg(not(feature = "tor"))]
fn unsupported() -> AetherError {
    AetherError::Other(
        "this build has no tor support; rebuild with `cargo build --release --features tor`".into(),
    )
}

#[cfg(not(feature = "tor"))]
pub async fn run_chain(_through: SocketAddr, _state: PathBuf) -> Result<()> {
    Err(unsupported())
}

#[cfg(not(feature = "tor"))]
pub async fn run_only(_listen: SocketAddr, _state: PathBuf) -> Result<()> {
    Err(unsupported())
}

#[cfg(not(feature = "tor"))]
pub async fn start_reverse(_state: PathBuf) -> Result<SocketAddr> {
    Err(unsupported())
}

#[cfg(feature = "tor")]
pub use with_tor::{run_chain, run_only, start_reverse};

#[cfg(feature = "tor")]
mod with_tor {
    use super::*;

    use std::collections::BTreeMap;
    use std::path::Path;
    use std::time::Duration;

    use arti_client::config::{BridgeConfigBuilder, CfgPath, TorClientConfigBuilder};
    use arti_client::{TorClient, TorClientConfig};
    use tokio::net::TcpListener;
    use tor_chanmgr::ProxyProtocol;
    use tor_rtcompat::PreferredRuntime;

    use crate::bridges::{Plan, Transport};

    pub type Client = std::sync::Arc<TorClient<PreferredRuntime>>;

    const BOOTSTRAP_RETRY: Duration = Duration::from_secs(10);
    const DIRECT_PROBE: Duration = Duration::from_secs(75);
    const BRIDGE_PROBE: Duration = Duration::from_secs(360);
    const STALL_LIMIT: Duration = Duration::from_secs(75);
    const STALL_CHECK: Duration = Duration::from_secs(5);
    const PROOF_LIMIT: Duration = Duration::from_secs(45);
    const WAVE_TRIES: usize = 2;
    // >>> AETHER-APP-PATCH tor-headway-is-a-whole-step
    /// Vielfaches des No-Headway-Limits, nach dem eine Sprosse in jedem Fall
    /// faellt -- auch wenn arti weiter Fortschritt meldet.
    const HARD_STALL_FACTOR: u32 = 3;
    // <<< AETHER-APP-PATCH tor-headway-is-a-whole-step
    const FOREVER: usize = usize::MAX;

    fn init_tracing() {
        use tracing_subscriber::filter::EnvFilter;

        let wanted = std::env::var("AETHER_TOR_LOG")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| match std::env::var("AETHER_LOG_LEVEL").as_deref() {
                Ok("debug") | Ok("trace") => "debug".to_string(),
                _ => "info".to_string(),
            });

        let filter = EnvFilter::try_new(&wanted).unwrap_or_else(|_| EnvFilter::new("info"));
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_target(true)
            .try_init();
    }
    const REVERSE_ATTEMPTS: usize = 3;

    fn direct_probe() -> Duration {
        std::env::var("AETHER_TOR_DIRECT_SECS")
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(DIRECT_PROBE)
    }

    fn manual_transports() -> Vec<Transport> {
        let mut grouped: Vec<Transport> = Vec::new();

        for (protocol, path) in transports() {
            let path = PathBuf::from(path);
            match grouped.iter_mut().find(|held| held.path == path) {
                Some(held) if !held.protocols.contains(&protocol) => held.protocols.push(protocol),
                Some(_) => {}
                None => grouped.push(Transport {
                    protocols: vec![protocol],
                    path,
                }),
            }
        }

        grouped
    }

    fn announce(plan: &Plan) {
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for line in &plan.lines {
            let kind = crate::bridges::transport_of(line).unwrap_or_else(|| "vanilla".to_string());
            *kinds.entry(kind).or_default() += 1;
        }

        if !plan.lines.is_empty() {
            let summary: Vec<String> = kinds
                .iter()
                .map(|(kind, count)| format!("{count} {kind}"))
                .collect();
            log::info!(
                "[+] tor has {} bridge(s) {}: {}",
                plan.lines.len(),
                plan.source,
                summary.join(", ")
            );
        }

        for transport in &plan.transports {
            log::info!(
                "[+] {} speaks {}",
                transport.path.display(),
                transport.protocols.join(", ")
            );
        }

        if !plan.missing.is_empty() {
            log::warn!(
                "[-] nothing here can speak {}; {}",
                plan.missing.join(", "),
                crate::bridges::install_hint()
            );
        }
    }

    async fn auto_plan(state: &Path) -> Plan {
        let (lines, source) = crate::bridges::fetch(state).await;
        let lines = crate::bridges::keep_reachable(lines).await;
        let plan = crate::bridges::plan(lines, source, manual_transports());
        announce(&plan);
        plan
    }

    fn build_config(
        state: &Path,
        through: Option<SocketAddr>,
        plan: Option<&Plan>,
        slot: Option<&str>,
    ) -> Result<TorClientConfig> {
        prepare_dir(state)?;
        let home = match slot {
            Some(name) => state.join("attempts").join(name),
            None => state.to_path_buf(),
        };
        let cache = home.join("cache");
        let data = home.join("state");
        prepare_dir(&home)?;
        prepare_dir(&cache)?;
        prepare_dir(&data)?;

        let mut builder = TorClientConfigBuilder::default();
        builder
            .storage()
            .cache_dir(CfgPath::new(cache.to_string_lossy().to_string()));
        builder
            .storage()
            .state_dir(CfgPath::new(data.to_string_lossy().to_string()));

        if let Some(proxy) = through {
            let parsed: ProxyProtocol = format!("socks5://{proxy}")
                .parse()
                .map_err(|e| AetherError::Other(format!("tor cannot dial through {proxy}: {e}")))?;
            builder.channel().outbound_proxy(parsed);
        }

        if let Some(plan) = plan {
            let mut taken = 0usize;
            for line in &plan.lines {
                match line.parse::<BridgeConfigBuilder>() {
                    Ok(bridge) => {
                        builder.bridges().bridges().push(bridge);
                        taken += 1;
                    }
                    Err(e) => log::warn!("[-] skipping a bridge tor cannot read: {e}"),
                }
            }

            if taken == 0 {
                return Err(AetherError::Other(
                    "none of the bridges could be read by tor".into(),
                ));
            }

            for transport in &plan.transports {
                let mut named = Vec::new();
                for protocol in &transport.protocols {
                    named.push(
                        protocol.parse().map_err(|e| {
                            AetherError::Other(format!("transport '{protocol}': {e}"))
                        })?,
                    );
                }

                let mut entry = arti_client::config::pt::TransportConfigBuilder::default();
                entry
                    .protocols(named)
                    .path(CfgPath::new(transport.path.to_string_lossy().to_string()))
                    .run_on_startup(true);
                builder.bridges().transports().push(entry);
            }
        }

        builder
            .build()
            .map_err(|e| AetherError::Other(format!("tor configuration: {e}")))
    }

    struct AbortOnDrop(tokio::task::AbortHandle);

    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    // >>> AETHER-APP-PATCH tor-wave-own-runtime
    /// Haelt die Laufzeit einer Sprosse und gibt sie ausserhalb des
    /// async-Kontexts frei.
    ///
    /// Die einzige Kopie ausser der des Clients. Der Client faellt zuerst und
    /// ist deshalb nie der Letzte; dieser Waechter faellt zuletzt und reicht
    /// seine Kopie an einen Thread weiter, wo Blockieren erlaubt ist.
    struct OwnRuntime(Option<PreferredRuntime>);

    impl OwnRuntime {
        /// Gebaut auf einem Thread OHNE Laufzeit -- niemals hier im async-Kontext.
        ///
        /// # Warum nicht einfach `PreferredRuntime::create()`
        ///
        /// Weil das aus einer async-Funktion heraus garantiert panikt, und zwar
        /// nicht bei uns, sondern eine Ebene tiefer. `create()` geht auf
        /// `tor_rtcompat::impls::tokio::create_runtime()`, und dort steht in
        /// 0.46.0:
        ///
        /// ```text
        /// impl From<async_executors::TokioTp> for TokioRuntimeHandle {
        ///     fn from(owner: async_executors::TokioTp) -> TokioRuntimeHandle {
        ///         let handle = owner.block_on(async { Handle::current() });
        /// ```
        ///
        /// `owner` ist die frisch gebaute Multi-Thread-Laufzeit, und
        /// `Runtime::block_on` auf einem Thread, der schon eine Laufzeit
        /// treibt, ist genau die Panik aus dem Log vom 2026-09-17:
        /// "Cannot start a runtime from within a runtime" bei
        /// `multi_thread/mod.rs:91`. Sie traf `Aether → Tor`, `Tor → Aether`
        /// und jede Sprosse dazwischen -- vier Versuche, vier Paniken, kein
        /// einziges Prozent Bootstrap.
        ///
        /// Ein eigener Thread hat keine Laufzeit betreten, dort ist
        /// `block_on` erlaubt. Die Laufzeit selbst gehoert danach dem
        /// zurueckgegebenen Handle, nicht dem Thread, der sie gebaut hat --
        /// der darf sofort sterben.
        async fn new() -> Result<Self> {
            let (tx, rx) = tokio::sync::oneshot::channel();
            std::thread::Builder::new()
                .name("tor-runtime-birth".to_string())
                .spawn(move || {
                    let made = PreferredRuntime::create()
                        .map_err(|e| AetherError::Other(format!("tor runtime: {e}")));
                    let _ = tx.send(made);
                })
                .map_err(|e| AetherError::Other(format!("tor runtime thread: {e}")))?;

            let runtime = rx.await.map_err(|_| {
                AetherError::Other("tor runtime: the thread that builds it vanished".to_string())
            })??;
            Ok(Self(Some(runtime)))
        }

        fn handle(&self) -> PreferredRuntime {
            self.0.clone().expect("the runtime is taken only in Drop")
        }
    }

    impl Drop for OwnRuntime {
        fn drop(&mut self) {
            if let Some(runtime) = self.0.take() {
                let sent = std::thread::Builder::new()
                    .name("tor-runtime-teardown".to_string())
                    .spawn(move || drop(runtime));
                if let Err(e) = sent {
                    log::warn!("[-] could not hand the tor runtime to a thread: {e}");
                }
            }
        }
    }
    // <<< AETHER-APP-PATCH tor-wave-own-runtime

    struct Sleeper {
        client: Client,
        keep: bool,
    }

    impl Drop for Sleeper {
        fn drop(&mut self) {
            if !self.keep {
                self.client.set_dormant(arti_client::DormantMode::Soft);
            }
        }
    }

    fn watch_progress(
        client: Client,
        label: &'static str,
        seen: std::sync::Arc<std::sync::atomic::AtomicU64>,
        // >>> AETHER-APP-PATCH tor-headway-is-a-whole-step
        // Die letzte erreichte ganze Stufe, damit die Fehlermeldung sagen kann,
        // WO es haengen blieb ("stuck at 15%") statt nur dass es haengt.
        stuck: std::sync::Arc<std::sync::atomic::AtomicU64>,
        // <<< AETHER-APP-PATCH tor-headway-is-a-whole-step
        started: std::time::Instant,
    ) -> AbortOnDrop {
        let task = tokio::spawn(async move {
            use futures::StreamExt;
            use std::sync::atomic::Ordering;

            let mut events = client.bootstrap_events();
            let mut shown = String::new();
            let mut best = 0u64;

            while let Some(status) = events.next().await {
                // >>> AETHER-APP-PATCH tor-headway-is-a-whole-step
                // Fortschritt ist ein GANZER Prozentpunkt, kein Epsilon.
                //
                // Vorher stand hier `* 1000.0`, also zaehlte jede Nachkomma-
                // regung von `as_frac()` als Fortschritt und setzte den
                // Waechter zurueck. Genau das passiert bei einem Consensus,
                // der nur teilweise ankommt ("Partial response" im Log vom
                // 2026-09-16): jedes Bruchstueck hebt den Bruch minimal, die
                // angezeigte Stufe bleibt "15%: connecting successfully", der
                // Log bleibt still -- und die Sprosse haengt ewig statt nach
                // 40 s zu fallen. Mit ganzen Prozent zaehlt nur echtes
                // Vorankommen.
                let reached = (status.as_frac().max(0.0) * 100.0).floor() as u64;
                if reached > best {
                    best = reached;
                    seen.store(started.elapsed().as_secs(), Ordering::Relaxed);
                    stuck.store(reached, Ordering::Relaxed);
                }
                // <<< AETHER-APP-PATCH tor-headway-is-a-whole-step

                let line = status.to_string();
                if line == shown {
                    continue;
                }
                shown = line.clone();

                if status.blocked().is_some() {
                    log::warn!("[-] tor {label}: {line}");
                } else {
                    log::info!("[*] tor {label}: {line}");
                }
            }
        });

        AbortOnDrop(task.abort_handle())
    }

    async fn boot(
        config: TorClientConfig,
        label: &'static str,
        stall: Option<Duration>,
    ) -> Result<Client> {
        use std::sync::atomic::{AtomicU64, Ordering};

        // >>> AETHER-APP-PATCH tor-wave-own-runtime
        // Eigene Laufzeit, damit diese Sprosse restlos verschwinden kann.
        //
        // Das Log vom 2026-09-16 zeigt fuenf `Memory quota tracking
        // initialised max=4.30 GiB` auf einer Maschine mit 5865 MB, drei
        // gleichzeitige Consensus-Zustandsmaschinen, und 43 Sekunden nach dem
        // Ende der snowflake-Sprosse noch deren `Rejected 2/2 as down` --
        // mitten in der obfs4-Sprosse, die sechs Bruecken hat.
        //
        // `set_dormant(Soft)` konnte das nicht verhindern; arti-client 0.46.0
        // beschreibt die Variante selbst als "Background tasks are suspended
        // ... Attempts to use the client will wake it back up again", und
        // einen laufenden Verzeichnis-Download weckt sie sofort wieder. Einen
        // echten Abschalter hat der Client in dieser Version nicht. Die
        // Laufzeit hat einen: tor-rtcompat 0.46.0 sagt zu `create()`, der
        // Rueckgabewert "will own the underlying Tokio runtime object, which
        // will be dropped when the last copy of this handle is freed".
        let grave = OwnRuntime::new().await?;

        // `create_unbootstrapped` (sync) wartet mit `std::thread::sleep` auf die
        // Dateisperren des Zustandsverzeichnisses und blockiert damit einen
        // Worker; die async-Variante tut dasselbe, ohne den Thread anzuhalten.
        // Bei drei Sprossen hintereinander auf demselben Verzeichnis ist das
        // kein Detail.
        let client = TorClient::with_runtime(grave.handle())
            .config(config)
            .create_unbootstrapped_async()
            .await
            .map_err(|e| AetherError::Other(format!("tor client: {e}")))?;
        // <<< AETHER-APP-PATCH tor-wave-own-runtime

        let started = std::time::Instant::now();
        let seen = std::sync::Arc::new(AtomicU64::new(0));
        // >>> AETHER-APP-PATCH tor-headway-is-a-whole-step
        let stuck = std::sync::Arc::new(AtomicU64::new(0));
        let _watcher = watch_progress(client.clone(), label, seen.clone(), stuck.clone(), started);
        // <<< AETHER-APP-PATCH tor-headway-is-a-whole-step
        let mut sleeper = Sleeper {
            client: client.clone(),
            keep: false,
        };

        let booted: Result<()> = match stall {
            None => client
                .bootstrap()
                .await
                .map_err(|e| AetherError::Other(format!("{e}"))),
            Some(limit) => {
                let handle = client.clone();
                let running = async move { handle.bootstrap().await };
                tokio::pin!(running);

                loop {
                    tokio::select! {
                        outcome = &mut running => {
                            break outcome.map_err(|e| AetherError::Other(format!("{e}")));
                        }
                        _ = tokio::time::sleep(STALL_CHECK) => {
                            let quiet = started
                                .elapsed()
                                .as_secs()
                                .saturating_sub(seen.load(Ordering::Relaxed));
                            // >>> AETHER-APP-PATCH tor-headway-is-a-whole-step
                            let at = stuck.load(Ordering::Relaxed);

                            // >>> AETHER-APP-PATCH tor-stall-check-speaks
                            // Damit Stille im Log etwas bedeutet. Alle 15s
                            // eine Zeile: laeuft diese Schleife noch, steht
                            // sie da. Steht sie nicht da, laeuft die Schleife
                            // nicht -- und dann ist es nicht das Netz.
                            let running_for = started.elapsed().as_secs();
                            if running_for % 15 < STALL_CHECK.as_secs() {
                                log::info!(
                                    "[i] tor {label}: still at {at}%, {quiet}s without a step, \
                                     {running_for}s in this attempt (falls at {}s, hard stop at {}s)",
                                    limit.as_secs(),
                                    limit.saturating_mul(HARD_STALL_FACTOR).as_secs()
                                );
                            }
                            // <<< AETHER-APP-PATCH tor-stall-check-speaks

                            if quiet >= limit.as_secs() {
                                break Err(AetherError::Other(format!(
                                    "no headway for {quiet}s (stuck at {at}%)"
                                )));
                            }

                            // Absolute Obergrenze, unabhaengig vom Fortschritts-
                            // signal. Der Waechter oben glaubt dem Bruch, den
                            // arti meldet; diese Grenze glaubt niemandem. Ohne
                            // sie konnte eine einzige Sprosse -- im Log vom
                            // 2026-09-16 obfs4 "go 1 of 2" -- das komplette
                            // Bruecken-Budget halten, sodass Sprosse 2 und
                            // snowflake nie an die Reihe kamen und die App fuenf
                            // Minuten lang stumm auf 15% stand.
                            if started.elapsed() >= limit.saturating_mul(HARD_STALL_FACTOR) {
                                break Err(AetherError::Other(format!(
                                    "no usable directory in {}s (stuck at {at}%)",
                                    started.elapsed().as_secs()
                                )));
                            }
                            // <<< AETHER-APP-PATCH tor-headway-is-a-whole-step
                        }
                    }
                }
            }
        };

        // >>> AETHER-APP-PATCH tor-wave-own-runtime
        // Ab hier gibt es zwei Wege, und nur einer behaelt den Client.
        let outcome = match booted {
            Ok(()) => prove(&client, label).await,
            Err(e) => Err(e),
        };

        if let Err(e) = outcome {
            // Freigegeben wird nicht hier, sondern wenn `grave` faellt -- das
            // passiert auch dann, wenn dieses Future von aussen abgebrochen
            // wird und diese Zeile nie erreicht.
            log::info!("[i] tor {label}: this attempt is closed, its runtime goes with it");
            return Err(e);
        }

        sleeper.keep = true;
        Ok(client)
    }

    fn proof_target() -> (String, u16) {
        let raw = std::env::var("AETHER_TOR_CHECK").unwrap_or_default();
        let raw = raw.trim();

        if raw.is_empty() {
            return ("check.torproject.org".to_string(), 443);
        }

        match raw.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), port.parse().unwrap_or(443)),
            None => (raw.to_string(), 443),
        }
    }

    async fn prove(client: &Client, label: &'static str) -> Result<()> {
        let (host, port) = proof_target();
        log::info!("[*] tor {label}: trying a way out through {host}:{port}");

        match tokio::time::timeout(PROOF_LIMIT, client.connect((host.as_str(), port))).await {
            Ok(Ok(_stream)) => {
                log::info!("[+] tor {label}: the way out is open");
                Ok(())
            }
            Ok(Err(e)) => Err(AetherError::Other(format!(
                "tor came up but could not reach {host}:{port}: {e}"
            ))),
            Err(_) => Err(AetherError::Other(format!(
                "tor came up but {host}:{port} stayed silent for {}s",
                PROOF_LIMIT.as_secs()
            ))),
        }
    }

    async fn stage(
        state: &Path,
        through: Option<SocketAddr>,
        plan: Option<&Plan>,
        label: &'static str,
        attempts: usize,
        deadline: Option<Duration>,
        stall: Option<Duration>,
        slot: Option<&str>,
    ) -> Result<Client> {
        let mut tried = 0usize;
        loop {
            tried += 1;
            let config = build_config(state, through, plan, slot)?;
            let run = boot(config, label, stall);

            let outcome = match deadline {
                Some(limit) => match tokio::time::timeout(limit, run).await {
                    Ok(result) => result,
                    Err(_) => Err(AetherError::Other(format!(
                        "no answer in {}s",
                        limit.as_secs()
                    ))),
                },
                None => run.await,
            };

            match outcome {
                Ok(client) => return Ok(client),
                Err(e) if tried >= attempts => return Err(e),
                Err(e) => {
                    log::warn!("[-] tor {label} failed: {e}; trying again in {BOOTSTRAP_RETRY:?}");
                    tokio::time::sleep(BOOTSTRAP_RETRY).await;
                }
            }
        }
    }

    async fn establish(state: &Path, through: Option<SocketAddr>, rounds: usize) -> Result<Client> {
        let policy = bridges();

        if let Bridges::Manual(lines) = &policy {
            let plan = crate::bridges::plan(lines.clone(), "given by hand", manual_transports());
            announce(&plan);
            if plan.is_empty() {
                return Err(AetherError::Other(format!(
                    "the bridges you gave need a pluggable transport that is not installed; {}",
                    crate::bridges::install_hint()
                )));
            }
            return stage(
                state,
                through,
                Some(&plan),
                "over your bridges",
                rounds,
                None,
                None,
                None,
            )
            .await;
        }

        let forced = matches!(policy, Bridges::Auto { forced: true });
        let may_fall_back = match (&policy, through) {
            (Bridges::Disabled, _) => false,
            (_, Some(_)) => forced,
            _ => true,
        };

        if !forced {
            let attempts = if may_fall_back { 1 } else { rounds };
            let deadline = may_fall_back.then(direct_probe);
            let tries = stage(
                state,
                through,
                None,
                "reaching the network",
                attempts,
                deadline,
                None,
                Some("direct"),
            );
            match tries.await {
                Ok(client) => return Ok(client),
                Err(e) if !may_fall_back => return Err(e),
                Err(e) => log::warn!("[-] tor could not get through on its own: {e}"),
            }
        }

        if through.is_some() {
            log::warn!("[-] a pluggable transport dials for itself, outside the tunnel");
        }

        let plan = auto_plan(state).await;
        if plan.is_empty() {
            return Err(AetherError::Other(format!(
                "tor is blocked on this network and no pluggable transport is installed, so no \
                 bridge can be used; {}",
                crate::bridges::install_hint()
            )));
        }

        if let Some(client) = try_each_transport(state, through, &plan).await? {
            return Ok(client);
        }

        stage(
            state,
            through,
            Some(&plan),
            "over bridges",
            rounds,
            None,
            None,
            Some("all"),
        )
        .await
    }

    fn bridge_probe() -> Duration {
        std::env::var("AETHER_TOR_BRIDGE_SECS")
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(BRIDGE_PROBE)
    }

    fn stall_limit() -> Duration {
        std::env::var("AETHER_TOR_STALL_SECS")
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(STALL_LIMIT)
    }

    async fn try_each_transport(
        state: &Path,
        through: Option<SocketAddr>,
        plan: &Plan,
    ) -> Result<Option<Client>> {
        let worked = crate::bridges::recall(state);
        // >>> AETHER-APP-PATCH bridge-order-per-country
        let waves = crate::bridges::waves(
            plan,
            worked.as_deref(),
            crate::bridges::last_country().as_deref(),
        );
        // <<< AETHER-APP-PATCH bridge-order-per-country
        if waves.is_empty() {
            return Ok(None);
        }

        let deadline = bridge_probe();
        let names: Vec<&str> = waves.iter().map(|(name, _)| name.as_str()).collect();
        log::info!("[*] tor will try {} in turn", names.join(", then "));

        // >>> AETHER-APP-PATCH tor-attempt-isolation
        // هر تلاش با یک state تازه شروع می‌شود.
        //
        // چیزی که لاگ ۲۰۲۶-۰۹-۱۶ نشان داد: پلهٔ snowflake که فقط **۲** پل دارد
        // گفت `No usable guards. Rejected 6/6 as down` — آن ۶ تا پل‌های obfs4ِ
        // پلهٔ قبل بودند. و پیش از آن:
        //
        //     tor_dirmgr: Another process is bootstrapping the directory.
        //                 Waiting till it finishes or exits.
        //
        // یعنی state میان تلاش‌ها مشترک بود: گاردهایی که یک تلاش «down» علامت
        // زده بود تلاش بعد را پیش از هر dial رد می‌کردند، و قفلِ دایرکتوریِ
        // کلاینتِ قبلی پلهٔ بعد را ~۳۰ ثانیه بی‌کار نگه می‌داشت. پاک‌کردن
        // پوشهٔ attempts در شروع، قفل و علامتِ جامانده از نشست قبلی را هم
        // برمی‌دارد. دانشِ مفید — «کدام ترانسپورت جواب داد» — در
        // bridges::remember/recall می‌ماند و دست‌نخورده است.
        let scratch = state.join("attempts");
        if scratch.exists() {
            if let Err(e) = std::fs::remove_dir_all(&scratch) {
                log::debug!("[i] could not clear {}: {e}", scratch.display());
            }
        }
        // <<< AETHER-APP-PATCH tor-attempt-isolation

        for (name, wave) in &waves {
            for round in 1..=WAVE_TRIES {
                let mut only = wave.clone();
                crate::bridges::shuffle(&mut only.lines);

                let where_to: Vec<String> = only
                    .lines
                    .iter()
                    .filter_map(|line| crate::bridges::address_of(line))
                    .map(|address| address.to_string())
                    .collect();

                log::info!(
                    "[*] {name}: go {round} of {WAVE_TRIES}, dropped after {}s without headway, \
                     over {}",
                    stall_limit().as_secs(),
                    if where_to.is_empty() {
                        format!("{} bridge(s)", only.lines.len())
                    } else {
                        where_to.join(", ")
                    }
                );

                let outcome = stage(
                    state,
                    through,
                    Some(&only),
                    "over bridges",
                    1,
                    Some(deadline),
                    Some(stall_limit()),
                    // >>> AETHER-APP-PATCH tor-attempt-isolation
                    // اسلات شاملِ شمارهٔ دور است، نه فقط نام ترانسپورت: دو دورِ
                    // یک ترانسپورت هم نباید state همدیگر را ببینند، وگرنه دورِ
                    // دوم همان ۶ گاردِ «down»‌شدهٔ دورِ اول را تحویل می‌گیرد و
                    // بی‌آنکه چیزی dial کند رد می‌شود — همان چیزی که در لاگ
                    // به‌صورت «Rejected 6/6 as down» دیده شد.
                    Some(&format!("{name}-{round}")),
                    // <<< AETHER-APP-PATCH tor-attempt-isolation
                )
                .await;

                match outcome {
                    Ok(client) => {
                        crate::bridges::remember(state, name);
                        log::info!("[+] {name} got through");
                        return Ok(Some(client));
                    }
                    Err(e) => log::warn!("[-] {name} did not get through: {e}"),
                }
            }
        }

        log::warn!("[-] no transport got through on its own; trying every bridge together");
        Ok(None)
    }

    async fn serve(listener: TcpListener, client: Client, kind: &'static str) -> Result<()> {
        crate::socks::serve_connector(listener, kind, move |host, port| {
            let client = client.clone();
            async move {
                client
                    .connect((host.as_str(), port))
                    .await
                    .map(|stream| {
                        use tokio_util::compat::FuturesAsyncReadCompatExt;
                        stream.compat()
                    })
                    .map_err(std::io::Error::other)
            }
        })
        .await
    }

    async fn wait_for_proxy(through: SocketAddr) {
        let mut announced = false;
        loop {
            if tokio::net::TcpStream::connect(through).await.is_ok() {
                return;
            }
            if !announced {
                log::info!("[*] tor is waiting for the tunnel proxy at {through}");
                announced = true;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    pub async fn run_chain(through: SocketAddr, state: PathBuf) -> Result<()> {
        init_tracing();
        let listen = listen_address();
        let listener = crate::socks::bind_listener("tor socks5", listen).await?;

        wait_for_proxy(through).await;
        log::info!("[*] bootstrapping tor through the tunnel at {through}");

        let client = establish(&state, Some(through), FOREVER).await?;
        log::info!("[+] tor is ready; {listen} leaves through tor, carried by the tunnel");

        serve(listener, client, "tor socks5").await
    }

    pub async fn run_only(listen: SocketAddr, state: PathBuf) -> Result<()> {
        init_tracing();
        let listener = crate::socks::bind_listener("tor socks5", listen).await?;

        log::info!("[*] bootstrapping tor with no tunnel underneath it");

        let client = establish(&state, None, FOREVER).await?;
        log::info!("[+] tor is ready; {listen} leaves through tor");

        serve(listener, client, "tor socks5").await
    }

    pub async fn start_reverse(state: PathBuf) -> Result<SocketAddr> {
        init_tracing();
        let listen = listen_address();
        let listener = crate::socks::bind_listener("tor socks5", listen).await?;

        log::info!("[*] bootstrapping tor; the tunnel will be dialled through it");

        let client = establish(&state, None, REVERSE_ATTEMPTS).await?;
        log::info!("[+] tor is ready; the tunnel goes out through {listen}");

        tokio::spawn(async move {
            if let Err(e) = serve(listener, client, "tor socks5").await {
                log::error!("[-] the tor proxy stopped: {e}");
            }
        });

        Ok(listen)
    }

    fn prepare_dir(path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_mode(0o700);
            std::fs::set_permissions(path, perms)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear() {
        std::env::remove_var("AETHER_TOR");
        std::env::remove_var("AETHER_TOR_DIR");
        std::env::remove_var("AETHER_TOR_BIND");
        std::env::remove_var("AETHER_TOR_BRIDGES");
        std::env::remove_var("AETHER_TOR_PT");
    }

    #[test]
    fn every_mode_has_a_spelling() {
        clear();
        assert_eq!(mode(), Mode::Off);
        for written in ["1", "on", "chain", "yes"] {
            std::env::set_var("AETHER_TOR", written);
            assert_eq!(mode(), Mode::Chain, "{written}");
        }
        for written in ["reverse", "rev", "warp-over-tor"] {
            std::env::set_var("AETHER_TOR", written);
            assert_eq!(mode(), Mode::Reverse, "{written}");
        }
        for written in ["only", "alone", "tor-only", "direct"] {
            std::env::set_var("AETHER_TOR", written);
            assert_eq!(mode(), Mode::Only, "{written}");
        }
        for written in ["0", "off", "false", "no"] {
            std::env::set_var("AETHER_TOR", written);
            assert_eq!(mode(), Mode::Off, "{written}");
        }
        clear();
    }

    #[test]
    fn the_state_dir_sits_beside_the_identity_file() {
        clear();
        assert_eq!(state_dir("aether.toml"), PathBuf::from("aether-tor"));
        assert_eq!(
            state_dir("/etc/aether/aether.toml"),
            PathBuf::from("/etc/aether/aether-tor")
        );
        std::env::set_var("AETHER_TOR_DIR", "/var/lib/aether-tor");
        assert_eq!(
            state_dir("aether.toml"),
            PathBuf::from("/var/lib/aether-tor")
        );
        clear();
    }

    #[test]
    fn the_tor_listener_has_a_default_of_its_own() {
        clear();
        assert_eq!(listen_address(), "127.0.0.1:1820".parse().unwrap());
        std::env::set_var("AETHER_TOR_BIND", "127.0.0.1:9150");
        assert_eq!(listen_address(), "127.0.0.1:9150".parse().unwrap());
        clear();
    }

    #[test]
    fn bridges_are_read_one_per_entry() {
        clear();
        assert!(bridge_lines().is_empty());
        std::env::set_var(
            "AETHER_TOR_BRIDGES",
            "obfs4 192.0.2.55:38114 316E64 cert=abc iat-mode=0 ; obfs4 198.51.100.25:443 7DD627 cert=def",
        );
        let lines = bridge_lines();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("obfs4 192.0.2.55"));
        assert!(lines[1].starts_with("obfs4 198.51.100.25"));
        clear();
    }

    #[test]
    fn a_transport_may_be_named_or_assumed_to_be_obfs4() {
        clear();
        assert!(transports().is_empty());
        std::env::set_var("AETHER_TOR_PT", "/usr/bin/lyrebird");
        assert_eq!(
            transports(),
            vec![("obfs4".to_string(), "/usr/bin/lyrebird".to_string())]
        );
        std::env::set_var(
            "AETHER_TOR_PT",
            "obfs4=/usr/bin/lyrebird;snowflake=/usr/bin/snowflake-client",
        );
        assert_eq!(
            transports(),
            vec![
                ("obfs4".to_string(), "/usr/bin/lyrebird".to_string()),
                (
                    "snowflake".to_string(),
                    "/usr/bin/snowflake-client".to_string()
                ),
            ]
        );
        clear();
    }
}
