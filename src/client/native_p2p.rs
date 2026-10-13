use super::*;
use hbb_common::{
    native_p2p::{self as p2p, transport::Session},
    tokio::time::timeout_at,
};

pub(super) async fn connect(
    peer: &str,
    endpoint: &str,
    key: &str,
    lc: Arc<RwLock<LoginConfigHandler>>,
) -> ResultType<
    Option<(
        Stream,
        bool,
        Option<Vec<u8>>,
        Option<KcpStream>,
        &'static str,
    )>,
> {
    if !crate::native_p2p::enabled() {
        return Ok(None);
    }
    let Some(_slot) = crate::native_p2p::slot() else {
        return Ok(None);
    };
    let pending = crate::native_p2p::Pending::new(&lc);
    tokio::select! {
        _ = pending.0.cancelled() => Err(crate::native_p2p::Cancelled.into()),
        result = connect_inner(peer, endpoint, key) => result,
    }
}

async fn connect_inner(
    peer: &str,
    endpoint: &str,
    key: &str,
) -> ResultType<
    Option<(
        Stream,
        bool,
        Option<Vec<u8>>,
        Option<KcpStream>,
        &'static str,
    )>,
> {
    let key = if key.is_empty() {
        crate::get_key(false).await
    } else {
        key.to_owned()
    };
    let server_key = crate::get_rs_pk(&key).context("native server key unavailable")?;
    let Some(session) =
        Session::begin(endpoint, crate::native_p2p::identity()?, server_key, peer).await?
    else {
        return Ok(None);
    };
    let signed_peer = session.ticket.signed_peer.clone();
    let (signed_id, _) = crate::decode_id_pk(&signed_peer, &server_key)?;
    hbb_common::anyhow::ensure!(signed_id == peer, "native target identity mismatch");
    let deadline = session.deadline;
    timeout_at(deadline, async move {
        let punched = session.punch(p2p::STUN_SERVER).await?;
        // The authenticated native probes already established this socket. The legacy
        // probe protocol mistakes a late native answer for the first KCP packet.
        let (kcp, mut stream) = KcpStream::connect(
            punched.socket.clone(),
            deadline.saturating_duration_since(Instant::now()),
        )
        .await?;
        let pk = Client::secure_connection(peer, signed_peer, &key, &mut stream).await?;
        hbb_common::anyhow::ensure!(stream.is_secured(), "native identity handshake failed");
        log::info!("native-p2p authenticated KCP connected to {}", punched.peer);
        let session = punched.session;
        tokio::spawn(async move {
            if let Err(error) = session.confirm_authenticated().await {
                log::trace!("native-p2p strategy report: {error}");
            }
        });
        Ok(Some((stream, true, pk, Some(kcp), "UDP-native")))
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

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
