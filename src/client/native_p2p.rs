use super::*;
use hbb_common::{native_p2p::{self as p2p, transport::Session, progress::{Observer, Progress, report}},
    tokio::time::timeout_at, tokio_util::sync::CancellationToken};
use crate::native_p2p::waiting::{self, Choice, Flow, Scope};

type Connection = (Stream, bool, Option<Vec<u8>>, Option<KcpStream>, &'static str);

/// CLI and diagnostics retain one bounded attempt and legacy fallback.
pub(super) async fn connect(peer: &str, endpoint: &str, key: &str,
    lc: Arc<RwLock<LoginConfigHandler>>) -> ResultType<Option<Connection>> {
    if !crate::native_p2p::enabled() { return Ok(None); }
    let Some(slot) = crate::native_p2p::slot() else { return Ok(None); };
    let pending = crate::native_p2p::Pending::new(&lc);
    let result = round(peer, endpoint, key, slot, &pending.0, None, None).await;
    if pending.0.is_cancelled() {
        if let Ok(Some(connection)) = result { dispose(connection).await; }
        return Err(crate::native_p2p::Cancelled.into());
    }
    result
}

pub(super) async fn connect_graphical(peer: &str, endpoint: &str, key: &str,
    interface: &impl Interface, scope: Scope) -> ResultType<Option<Connection>> {
    if !crate::native_p2p::enabled() { return Ok(None); }
    let flow = scope.window.begin(scope.mapping);
    interface.msgbox("native-p2p", "", &flow.id.to_string(), "");
    let result = continuous(peer, endpoint, key, &scope, &flow).await;
    let choice = scope.window.finish(&flow);
    interface.msgbox("native-p2p-end", "", &flow.id.to_string(), "");
    if choice == Choice::Cancel { return Err(crate::native_p2p::Cancelled.into()); }
    if choice == Choice::Relay {
        // Session memory only. peer_relay is intentionally not modified/persisted.
        let lc = interface.get_lch();
        let mut lc = lc.write().unwrap();
        lc.force_relay = true;
        lc.policy_relay = true;
    }
    result
}

async fn continuous(peer: &str, endpoint: &str, key: &str, scope: &Scope, flow: &Arc<Flow>) -> ResultType<Option<Connection>> {
    drive(scope, flow, |slot, observer| round(peer, endpoint, key, slot, &flow.cancel, observer, Some(flow))).await
}

async fn drive<F, Fut>(scope: &Scope, flow: &Arc<Flow>, mut attempt_round: F) -> ResultType<Option<Connection>>
where F: FnMut(hbb_common::tokio::sync::OwnedSemaphorePermit, Observer) -> Fut,
    Fut: std::future::Future<Output = ResultType<Option<Connection>>> {
    loop {
        if flow.choice() != Choice::Pending { return outcome(flow); }
        flow.stage("slot");
        let slot = tokio::select! {
            biased;
            _ = flow.cancel.cancelled() => return outcome(flow),
            _ = settings_changed() => { flow.legacy(); return outcome(flow); }
            slot = crate::native_p2p::acquire() => slot.context("native attempt semaphore closed")?,
        };
        let (generation, observer) = flow.start_round();
        let attempt = attempt_round(slot, observer);
        tokio::pin!(attempt);
        let result = tokio::select! {
            result = &mut attempt => result,
            _ = settings_changed() => {
                flow.legacy();
                // Cooperatively stop, then await explicit cleanup before returning.
                attempt.await
            }
        };
        if let Ok(Some(connection)) = result {
            if scope.window.commit_direct(flow) {
                flow.finish_round("P2P identity verified", false);
                return Ok(Some(connection));
            }
            dispose(connection).await;
            return outcome(flow);
        }
        if flow.choice() != Choice::Pending { return outcome(flow); }
        let (reason, parked) = match result {
            // Older coordinators also use Unsupported for offline/unregistered peers.
            Ok(None) => ("Peer P2P capability is unknown; waiting for registration".to_owned(), false),
            Err(error) => { let text = error.to_string(); let terminal = waiting::terminal_reason(&text); (text, terminal) },
            _ => unreachable!(),
        };
        hbb_common::throttled_log!(Duration::from_secs(1), debug, "native-p2p round {generation} ended (waiting_for_choice={parked}): {reason}");
        flow.finish_round(&reason, parked);
        tokio::select! {
            biased;
            _ = flow.cancel.cancelled() => return outcome(flow),
            _ = settings_changed() => { flow.legacy(); return outcome(flow); }
            _ = async { if parked { std::future::pending::<()>().await; } else { tokio::time::sleep(waiting::backoff(generation)).await; } } => {}
        }
    }
}

fn outcome(flow: &Flow) -> ResultType<Option<Connection>> {
    match flow.choice() {
        Choice::Relay | Choice::Legacy => Ok(None),
        _ => Err(crate::native_p2p::Cancelled.into()),
    }
}
async fn settings_changed() {
    while crate::native_p2p::enabled() { tokio::time::sleep(Duration::from_millis(250)).await; }
}
async fn dispose(connection: Connection) {
    let (stream, _, _, kcp, _) = connection;
    drop(stream);
    if let Some(kcp) = kcp { kcp.close_native().await; }
}

async fn round(peer: &str, endpoint: &str, key: &str,
    slot: hbb_common::tokio::sync::OwnedSemaphorePermit, cancel: &CancellationToken,
    observer: Observer, flow: Option<&Flow>) -> ResultType<Option<Connection>> {
    // Reserve teardown within the absolute 60 second round, including Android withdrawal.
    let deadline = Instant::now() + Duration::from_secs(p2p::ATTEMPT_SECONDS) - Duration::from_millis(1500);
    report(&observer, Progress::Stage("prepare"));
    let key = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(crate::native_p2p::Cancelled.into()),
        value = timeout_at(deadline, async { if key.is_empty() { crate::get_key(false).await } else { key.to_owned() } }) => value?,
    };
    let server_key = crate::get_rs_pk(&key).context("native server key unavailable")?;
    #[cfg(not(target_os = "android"))]
    let _slot = slot;
    #[cfg(not(target_os = "android"))]
    let session = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(crate::native_p2p::Cancelled.into()),
        session = Session::begin_with_timeout(endpoint, crate::native_p2p::identity()?, server_key, peer, Duration::from_secs(5).min(deadline.saturating_duration_since(Instant::now()))) => session?,
    };
    #[cfg(not(target_os = "android"))]
    let Some(mut session) = session else { return Ok(None); };
    #[cfg(target_os = "android")]
    let Some((mut session, mut registration)) = crate::native_p2p::android::begin_controlled(endpoint, server_key, peer, slot, cancel, deadline.saturating_duration_since(Instant::now())).await?
    else { return Ok(None); };
    if let Some(flow) = flow { flow.limit(session.deadline.min(deadline + Duration::from_millis(1500))); }
    session.deadline = session.deadline.min(deadline);
    let result = connected_round(peer, &key, &server_key, &session, cancel, observer).await;
    // No new round, relay or slot handoff until local tasks and leases are settled.
    session.close().await;
    #[cfg(target_os = "android")]
    registration.close().await;
    result.map(Some)
}

async fn connected_round(peer: &str, key: &str, server_key: &hbb_common::sodiumoxide::crypto::sign::PublicKey,
    session: &Session, cancel: &CancellationToken, observer: Observer) -> ResultType<Connection> {
    let signed_peer = session.ticket.signed_peer.clone();
    let (signed_id, _) = crate::decode_id_pk(&signed_peer, server_key)?;
    hbb_common::anyhow::ensure!(signed_id == peer, "native target identity mismatch");
    let deadline = session.deadline;
    #[cfg(not(target_os = "android"))]
    let interfaces = p2p::interfaces::physical().unwrap_or_default();
    #[cfg(target_os = "android")]
    let environment = crate::native_p2p::android::environment().context("Android native P2P environment unavailable")?;
    #[cfg(target_os = "android")]
    let interfaces = environment.interfaces.clone();
    let (socket, remote, _) = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(crate::native_p2p::Cancelled.into()),
        result = session.probe(p2p::STUN_SERVER, &interfaces, observer.clone()) => result?,
    };
    report(&observer, Progress::Stage("kcp"));
    let (kcp, mut stream) = KcpStream::native(socket, deadline.saturating_duration_since(Instant::now()), false,
        async { tokio::select! { _ = cancel.cancelled() => {}, _ = session.wait_for_cancellation() => {} } }).await?;
    report(&observer, Progress::Stage("secure"));
    let secured = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(crate::native_p2p::Cancelled.into()),
        value = timeout_at(deadline, Client::secure_connection(peer, signed_peer, key, &mut stream)) => match value { Ok(result) => result, Err(error) => Err(error.into()) },
    };
    match secured {
        Ok(pk) if stream.is_secured() => {
            log::info!("native-p2p authenticated KCP connected to {remote}");
            if let Err(error) = session.confirm_authenticated().await { log::trace!("native strategy report: {error}"); }
            report(&observer, Progress::Stage("authenticated"));
            Ok((stream, true, pk, Some(kcp), "UDP-native"))
        }
        result => {
            drop(stream);
            kcp.close_native().await;
            Err(result.err().unwrap_or_else(|| hbb_common::anyhow::anyhow!("native identity handshake failed")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn closing_during_relay_cleanup_does_not_start_fallback() {
        for (remove_mapping, after_cleanup) in [(false, false), (true, false), (false, true), (true, true)] {
            let window = Arc::new(waiting::Context::default());
            let scope = Scope { mapping: Some(window.mapping(12345)), window };
            let flow = scope.window.begin(scope.mapping);
            let stop = || {
                if remove_mapping { scope.window.cancel_mapping(12345); }
                else { scope.window.close(); }
            };
            let attempt = drive(&scope, &flow, |slot, _| async {
                let _slot = slot;
                flow.cancel.cancelled().await;
                tokio::time::sleep(Duration::from_millis(10)).await;
                Err(crate::native_p2p::Cancelled.into())
            });
            let close = async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                assert!(scope.window.relay(flow.id));
                if !after_cleanup { stop(); }
            };
            let (result, _) = tokio::join!(attempt, close);
            if after_cleanup {
                assert!(result.unwrap().is_none());
                stop();
            } else {
                assert!(result.err().is_some_and(|e| e.is::<crate::native_p2p::Cancelled>()));
            }
            assert_eq!(scope.window.finish(&flow), Choice::Cancel);
            assert!(crate::native_p2p::slot().is_some());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn unknown_capability_and_transient_rounds_retry_and_release_the_slot() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let scope = Scope { window: Default::default(), mapping: None };
        let flow = scope.window.begin(None);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let (_peer, _) = listener.accept().await.unwrap();
        let success = Mutex::new(Some(Stream::Tcp(hbb_common::tcp::FramedStream::from(socket, address))));
        let count = AtomicUsize::new(0);
        let times = Mutex::new(Vec::new());
        let result = drive(&scope, &flow, |slot, observer| {
            let n = count.fetch_add(1, Ordering::SeqCst);
            times.lock().unwrap().push(Instant::now());
            let success = &success;
            async move {
                let _slot = slot;
                report(&observer, Progress::Stage("probe"));
                tokio::time::sleep(Duration::from_secs(1)).await;
                if n == 999 { return Ok(Some((success.lock().unwrap().take().unwrap(), true, None, None, "test"))); }
                if n == 0 { return Ok(None); }
                Err(hbb_common::anyhow::anyhow!("network timeout; support unknown"))
            }
        }).await;
        assert!(result.unwrap().is_some());
        assert_eq!(flow.choice(), Choice::Direct);
        assert!(!scope.window.relay(flow.id));
        assert_eq!(count.load(Ordering::SeqCst), 1000);
        let times = times.lock().unwrap();
        assert_eq!(times[1] - times[0], Duration::from_secs(3));
        assert_eq!(times[2] - times[1], Duration::from_secs(6));
        assert_eq!(times[999] - times[998], Duration::from_secs(11));
        assert!(crate::native_p2p::slot().is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn explicit_unsupported_waits_without_retry_until_cancelled() {
        let scope = Scope { window: Default::default(), mapping: None };
        let flow = scope.window.begin(None);
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let drive = drive(&scope, &flow, |slot, _| {
            attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                drop(slot);
                Err(hbb_common::anyhow::anyhow!(p2p::progress::Reason { code: "unsupported".into(),
                    message: "Native P2P is disabled on the peer".into(), local: None, peer: None }.encode()))
            }
        });
        let cancel = async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            assert!(crate::native_p2p::slot().is_some());
            scope.window.cancel_flow(flow.id);
        };
        let (result, _) = tokio::join!(drive, cancel);
        assert!(result.err().unwrap().is::<crate::native_p2p::Cancelled>());
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn hundred_real_kcp_cancellations_release_all_socket_references() {
        for accept in [false, true] {
            for _ in 0..50 {
                let socket = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
                let peer = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                socket.connect(peer.local_addr().unwrap()).await.unwrap();
                let result = KcpStream::native(socket.clone(), Duration::from_secs(60), accept,
                    tokio::time::sleep(Duration::from_millis(2))).await;
                assert!(result.is_err());
                assert_eq!(Arc::strong_count(&socket), 1, "native KCP pump leaked a socket");
            }
        }
    }

    #[tokio::test]
    async fn legacy_policy_skips_native_coordination() {
        use base::config::keys;
        for (option, value) in [
            (keys::OPTION_ENABLE_NATIVE_P2P, "N"),
            (keys::OPTION_PROXY_URL, "socks5://127.0.0.1:9"),
            (keys::OPTION_ALLOW_WEBSOCKET, "Y"),
        ] {
            let previous = hbb_common::config::OVERWRITE_SETTINGS
                .write().unwrap().insert(option.into(), value.into());
            let result = tokio::time::timeout(
                Duration::from_millis(100),
                connect("peer", "invalid endpoint", "invalid key", Default::default()),
            ).await;
            let mut settings = hbb_common::config::OVERWRITE_SETTINGS.write().unwrap();
            if let Some(value) = previous { settings.insert(option.into(), value); }
            else { settings.remove(option); }
            // Invalid coordination inputs would fail if the legacy policy reached them.
            assert!(result.unwrap().unwrap().is_none(), "{option}");
        }
    }

    #[tokio::test]
    async fn native_kcp_ignores_late_probe_replies() {
        let easy = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let hard = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
        easy.connect(hard.local_addr().unwrap()).await.unwrap();
        hard.connect(easy.local_addr().unwrap()).await.unwrap();
        let accept = KcpStream::accept(easy, Duration::from_secs(3), None);
        let connect = async move {
            // Mode 2 returns on the first authenticated answer; repeated answers can
            // still arrive before the HardNAT caller starts its KCP handshake.
            let mut answer = vec![0; 89];
            answer[..8].copy_from_slice(b"RDP2PV1\0");
            for _ in 0..3 {
                hard.send(&answer).await.unwrap();
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            KcpStream::connect(hard, Duration::from_secs(3)).await
        };
        let (accepted, connected) = tokio::join!(accept, connect);
        let (_receiver, mut input) = accepted.unwrap();
        let (_caller, mut output) = connected.unwrap();
        output.send_bytes(hbb_common::bytes::Bytes::from_static(b"native-kcp")).await.unwrap();
        let received = input.next_timeout(1000).await.unwrap().unwrap();
        assert_eq!(&received[..], b"native-kcp");
    }
}
