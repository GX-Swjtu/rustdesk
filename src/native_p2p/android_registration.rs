//! On-demand identity registration for a controller with screen sharing stopped.
use super::*;
use hbb_common::{protobuf::Message, rendezvous_proto::*};
use std::{collections::HashMap, time::Duration};
use tokio::{net::UdpSocket, time::timeout};

hbb_common::lazy_static::lazy_static! {
    static ref REGISTRATIONS: Mutex<HashMap<String, Instant>> = Mutex::new(HashMap::new());
}

pub(crate) fn record(endpoint: &str, registration: &RendezvousMessage) {
    let mut entries = REGISTRATIONS.lock().unwrap();
    entries.retain(|_, seen| seen.elapsed() < Duration::from_secs(30));
    if matches!(&registration.union, Some(rendezvous_message::Union::RegisterPeer(r)) if r.native_p2p_version == p2p::VERSION) {
        entries.insert(endpoint.into(), Instant::now());
    } else {
        entries.remove(endpoint);
    }
}

fn active(endpoint: &str) -> bool {
    REGISTRATIONS.lock().unwrap().get(endpoint)
        .map_or(false, |seen| seen.elapsed() < Duration::from_secs(30))
}

pub(crate) struct Registration {
    socket: Option<Arc<UdpSocket>>,
    endpoint: String,
    identity: p2p::transport::Identity,
    slot: Option<OwnedSemaphorePermit>,
}

fn advertisement(identity: &p2p::transport::Identity, enabled: bool) -> Result<Vec<u8>> {
    let mut message = RendezvousMessage::new();
    message.set_register_peer(RegisterPeer {
        id: identity.id.clone(), serial: Config::get_serial(),
        native_p2p_version: if enabled { p2p::VERSION } else { 0 },
        native_p2p_capability: protocol::encode(&protocol::request(&identity.id,
            Action::Advertise { capability: p2p::CAPABILITY.into(), enabled })?, &identity.key)?.into(),
        ..Default::default()
    });
    Ok(message.write_to_bytes()?)
}

pub(crate) async fn begin(
    endpoint: &str,
    server_key: sign::PublicKey,
    peer: &str,
    slot: OwnedSemaphorePermit,
) -> Result<Option<(Session, Registration)>> {
    let deadline = Instant::now() + Duration::from_secs(p2p::ATTEMPT_SECONDS);
    let limit = Duration::from_secs(5);
    timeout(limit, async {
        let registration = prepare(endpoint, slot).await?;
        let Some(mut session) = Session::begin_with_timeout(endpoint, registration.identity.clone(), server_key, peer, limit).await?
        else { return Ok(None); };
        session.deadline = session.deadline.min(deadline);
        Ok(Some((session, registration)))
    }).await?
}

async fn prepare(endpoint: &str, slot: OwnedSemaphorePermit) -> Result<Registration> {
    let identity = super::super::identity()?;
    if active(endpoint) {
        return Ok(Registration { socket: None, endpoint: endpoint.into(), identity, slot: Some(slot) });
    }
    let address = tokio::net::lookup_host(endpoint).await?
        .find(std::net::SocketAddr::is_ipv4).context("no IPv4 coordinator")?;
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    socket.connect(address).await?;
    let lease = Registration { socket: Some(socket.clone()), endpoint: endpoint.into(), identity, slot: Some(slot) };
    socket.send(&advertisement(&lease.identity, true)?).await?;
    let mut data = [0u8; 4096];
    let n = socket.recv(&mut data).await?;
    let reply = RendezvousMessage::parse_from_bytes(&data[..n])?;
    let Some(rendezvous_message::Union::RegisterPeerResponse(response)) = reply.union
    else { hbb_common::anyhow::bail!("Android native capability registration declined"); };
    if response.request_pk {
        let mut message = RendezvousMessage::new();
        message.set_register_pk(RegisterPk {
            id: lease.identity.id.clone(), uuid: hbb_common::get_uuid().into(),
            pk: lease.identity.key.public_key().as_ref().to_vec().into(),
            no_register_device: Config::no_register_device(), ..Default::default()
        });
        socket.send(&message.write_to_bytes()?).await?;
        let n = socket.recv(&mut data).await?;
        let reply = RendezvousMessage::parse_from_bytes(&data[..n])?;
        hbb_common::anyhow::ensure!(matches!(reply.union,
            Some(rendezvous_message::Union::RegisterPkResponse(r)) if r.result.enum_value_or_default() == register_pk_response::Result::OK),
            "Android native device registration declined");
        socket.send(&advertisement(&lease.identity, true)?).await?;
        let n = socket.recv(&mut data).await?;
        let reply = RendezvousMessage::parse_from_bytes(&data[..n])?;
        hbb_common::anyhow::ensure!(matches!(reply.union,
            Some(rendezvous_message::Union::RegisterPeerResponse(r)) if !r.request_pk), "Android native capability registration declined");
    }
    Ok(lease)
}

impl Drop for Registration {
    fn drop(&mut self) {
        let Some(socket) = self.socket.take() else { return; };
        // Restore a screen-sharing registration if it came online during this attempt.
        refresh_registration();
        if active(&self.endpoint) { return; }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else { return; };
        let slot = self.slot.take();
        let bytes = advertisement(&self.identity, false);
        runtime.spawn(async move {
            // A late withdrawal must not disable a subsequent native attempt.
            let _slot = slot;
            let result = async {
                let bytes = bytes?;
                timeout(Duration::from_secs(1), async {
                    socket.send(&bytes).await?;
                    let mut data = [0u8; 4096];
                    loop {
                        let n = socket.recv(&mut data).await?;
                        let reply = RendezvousMessage::parse_from_bytes(&data[..n])?;
                        if matches!(reply.union, Some(rendezvous_message::Union::RegisterPeerResponse(_))) { break; }
                    }
                    Ok::<_, hbb_common::anyhow::Error>(())
                }).await??;
                Ok::<_, hbb_common::anyhow::Error>(())
            }.await;
            if let Err(error) = result { log::trace!("Android native capability cleanup: {error}"); }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rapid_callers_reuse_device_key_and_withdraw_on_drop() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let endpoint = server.local_addr().unwrap().to_string();
        let listener = tokio::spawn(async move {
            let mut data = [0u8; 4096];
            let (mut key, mut registrations, mut withdrawals) = (None, 0, 0);
            while withdrawals < 4 {
                let (n, address) = timeout(Duration::from_secs(3), server.recv_from(&mut data)).await.unwrap().unwrap();
                let mut reply = RendezvousMessage::new();
                match RendezvousMessage::parse_from_bytes(&data[..n]).unwrap().union {
                    Some(rendezvous_message::Union::RegisterPk(r)) => {
                        registrations += 1;
                        key = Some(sign::PublicKey::from_slice(&r.pk).unwrap());
                        // hbbs permits only three key registrations in a six-second window.
                        let result = if registrations <= 3 { register_pk_response::Result::OK }
                            else { register_pk_response::Result::TOO_FREQUENT };
                        reply.set_register_pk_response(RegisterPkResponse { result: result.into(), ..Default::default() });
                    }
                    Some(rendezvous_message::Union::RegisterPeer(r)) => {
                        if let Some(key) = &key {
                            let request: protocol::Request = protocol::decode(&r.native_p2p_capability, key).unwrap();
                            let Action::Advertise { capability, enabled } = request.action else { panic!("expected advertisement"); };
                            assert_eq!(capability, p2p::CAPABILITY);
                            if !enabled { withdrawals += 1; }
                        }
                        reply.set_register_peer_response(RegisterPeerResponse { request_pk: key.is_none(), ..Default::default() });
                    }
                    _ => panic!("unexpected registration message"),
                }
                server.send_to(&reply.write_to_bytes().unwrap(), address).await.unwrap();
            }
            registrations
        });
        let permits = Arc::new(Semaphore::new(1));
        for _ in 0..4 {
            let slot = timeout(Duration::from_secs(2), permits.clone().acquire_owned()).await.unwrap().unwrap();
            let lease = timeout(Duration::from_secs(1), prepare(&endpoint, slot)).await.unwrap().unwrap();
            assert!(lease.socket.is_some());
            drop(lease);
        }
        assert_eq!(listener.await.unwrap(), 1);

        // An active sharing mediator owns its registration address; don't replace it.
        let identity = super::super::super::identity().unwrap();
        let message = RendezvousMessage::parse_from_bytes(&advertisement(&identity, true).unwrap()).unwrap();
        record(&endpoint, &message);
        let lease = prepare(&endpoint, permits.acquire_owned().await.unwrap()).await.unwrap();
        assert!(lease.socket.is_none());
    }

    #[tokio::test]
    async fn registration_and_capability_share_five_second_preparation_budget() {
        for delay_ms in [1500, 5500] {
            let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = tcp.local_addr().unwrap();
            let udp = UdpSocket::bind(address).await.unwrap();
            let tcp_task = tokio::spawn(async move {
                let (_stream, _) = tcp.accept().await.unwrap();
                std::future::pending::<()>().await;
            });
            let udp_task = tokio::spawn(async move {
                let mut data = [0u8; 4096];
                let (_, peer) = udp.recv_from(&mut data).await.unwrap();
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                let mut reply = RendezvousMessage::new();
                reply.set_register_peer_response(RegisterPeerResponse::default());
                let bytes = reply.write_to_bytes().unwrap();
                udp.send_to(&bytes, peer).await.unwrap();
                let (_, peer) = udp.recv_from(&mut data).await.unwrap();
                udp.send_to(&bytes, peer).await.unwrap();
            });
            let permits = Arc::new(Semaphore::new(1));
            let key = sign::gen_keypair().0;
            let started = Instant::now();
            let result = begin(&address.to_string(), key, "legacy-peer", permits.clone().acquire_owned().await.unwrap()).await;
            assert!(result.is_err());
            assert!(started.elapsed() >= Duration::from_secs(5), "unknown server still has a shorter preparation budget");
            assert!(started.elapsed() < Duration::from_millis(5500), "registration delay {delay_ms} extended the shared preparation budget");
            // Cleanup must release the attempt slot even when registration times out.
            drop(timeout(Duration::from_secs(2), permits.acquire_owned()).await.unwrap().unwrap());
            timeout(Duration::from_secs(2), udp_task).await.unwrap().unwrap();
            tcp_task.abort();
            let _ = tcp_task.await;
        }
    }

    #[tokio::test]
    async fn first_connection_accepts_delayed_authenticated_server() {
        use hbb_common::{bytes_codec::BytesCodec, futures::{SinkExt, StreamExt}, tokio_util::codec::Framed};
        let device_key = super::super::super::identity().unwrap().key.public_key();
        let (server_key, secret) = sign::gen_keypair();
        let wrong_key = sign::gen_keypair().0;
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = tcp.local_addr().unwrap();
        let identity = super::super::super::identity().unwrap();
        record(&address.to_string(), &RendezvousMessage::parse_from_bytes(&advertisement(&identity, true).unwrap()).unwrap());
        let tcp_task = tokio::spawn(async move {
            loop {
                let (stream, _) = tcp.accept().await.unwrap();
                let mut stream = Framed::new(stream, BytesCodec::new());
                let message = RendezvousMessage::parse_from_bytes(&stream.next().await.unwrap().unwrap()).unwrap();
                let request: protocol::Request = protocol::decode(&message.native_p2p_v1().payload, &device_key).unwrap();
                let reply = if let Action::Begin { box_pk, target, .. } = request.action {
                    tokio::time::sleep(Duration::from_millis(1200)).await;
                    protocol::Reply::Ticket(protocol::Ticket { session: protocol::id(), expires: protocol::now().unwrap() + 45,
                        secret_box: protocol::seal(&[1; 32], &box_pk).unwrap(), peer_id: target, signed_peer: vec![],
                        initiator: true, capability: p2p::CAPABILITY.into() })
                } else { protocol::Reply::Closed };
                let response = protocol::Response { nonce: request.nonce, time: protocol::now().unwrap(), reply };
                // Timeout/cancellation may have closed the caller before this delayed reply.
                let _ = stream.send(p2p::transport::envelope(protocol::encode(&response, &secret).unwrap()).write_to_bytes().unwrap().into()).await;
            }
        });
        let permits = Arc::new(Semaphore::new(1));
        for (key, succeeds) in [(server_key, true), (wrong_key, false)] {
            let slot = timeout(Duration::from_secs(2), permits.clone().acquire_owned()).await.unwrap().unwrap();
            let started = Instant::now();
            let result = begin(&address.to_string(), key, "peer", slot).await;
            if succeeds {
                let (session, registration) = result.unwrap().unwrap();
                assert!(session.deadline <= started + Duration::from_secs(p2p::ATTEMPT_SECONDS) + Duration::from_millis(100));
                drop((session, registration));
            } else {
                assert!(result.is_err());
                assert!(started.elapsed() < Duration::from_secs(3), "invalid signature did not fall back promptly");
            }
        }
        drop(timeout(Duration::from_secs(2), permits.acquire_owned()).await.unwrap().unwrap());
        tcp_task.abort();
        let _ = tcp_task.await;
        record(&address.to_string(), &RendezvousMessage::new());
    }
}
