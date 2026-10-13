use hbb_common::{anyhow, base64, protobuf, sodiumoxide, tokio, log};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use protobuf::Message;
use hbb_common::native_p2p::{
    self as p2p,
    protocol::{self, Action, Invite},
    transport::{Identity, Session},
    wire::rendezvous::*,
};
use sodiumoxide::{crypto::sign, randombytes::randombytes};
use std::{sync::Arc, time::Duration};
use tokio::{
    net::UdpSocket,
    task::JoinSet,
    time::{timeout, timeout_at, Instant},
};

#[path = "../../../../src/kcp_stream.rs"]
mod kcp_stream;
mod proof;

pub fn get_kcp_cc_enabled() -> bool { false }

fn report(event: &str, value: serde_json::Value) {
    println!("{}", serde_json::json!({"event": event, "data": value}));
}

struct DiagnosticLog;

impl log::Log for DiagnosticLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool { metadata.level() <= log::Level::Info }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) { report("log", serde_json::json!(record.args().to_string())); }
    }
    fn flush(&self) {}
}

async fn register(socket: &UdpSocket, identity: &Identity) -> Result<()> {
    let pk = identity.key.public_key();
    let mut message = RendezvousMessage::new();
    message.set_register_pk(RegisterPk {
        id: identity.id.clone(),
        uuid: randombytes(16).into(),
        pk: pk.as_ref().to_vec().into(),
        ..Default::default()
    });
    socket.send(&message.write_to_bytes()?).await?;
    let mut data = [0u8; 4096];
    let n = timeout(Duration::from_secs(3), socket.recv(&mut data)).await??;
    let response = RendezvousMessage::parse_from_bytes(&data[..n])?;
    ensure!(
        matches!(response.union, Some(rendezvous_message::Union::RegisterPkResponse(r)) if r.result.enum_value_or_default() == register_pk_response::Result::OK),
        "key registration failed"
    );
    advertise(socket, identity).await
}

async fn advertise(socket: &UdpSocket, identity: &Identity) -> Result<()> {
    let capability = protocol::encode(
        &protocol::request(
            &identity.id,
            Action::Advertise {
                capability: p2p::CAPABILITY.into(),
                enabled: true,
            },
        )?,
        &identity.key,
    )?;
    let mut message = RendezvousMessage::new();
    message.set_register_peer(RegisterPeer {
        id: identity.id.clone(),
        native_p2p_version: p2p::VERSION,
        native_p2p_capability: capability.into(),
        ..Default::default()
    });
    socket.send(&message.write_to_bytes()?).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    p2p::init()?;
    log::set_logger(&DiagnosticLog).map_err(|_| anyhow::anyhow!("diagnostic logger already set"))?;
    log::set_max_level(log::LevelFilter::Info);
    let args: Vec<String> = std::env::args().collect();
    let role = args.get(1).map(String::as_str).unwrap_or("help");
    if role == "discover" {
        let (socket, addresses) =
            p2p::stun::discover_profile(args.get(2).map(String::as_str).unwrap_or(p2p::STUN_SERVER))
                .await?;
        report(
            "mapping",
            serde_json::json!({"local":socket.local_addr()?, "observations":addresses, "profile":p2p::strategy::classify(&p2p::candidates::Candidates { public: addresses.clone(), local: vec![], direct: None })?}),
        );
        return Ok(());
    }
    ensure!(matches!(role, "listen" | "connect"), "usage: native-p2p-probe discover [stun] | listen SERVER PUBLIC_KEY | connect SERVER PUBLIC_KEY TARGET_ID");
    let endpoint = args.get(2).context("missing server")?;
    let key = STANDARD.decode(args.get(3).context("missing server public key")?.trim())?;
    let server = sign::PublicKey::from_slice(&key).context("invalid server public key")?;
    let (_, key) = sign::gen_keypair();
    let identity = Identity {
        id: format!("p2p{}", protocol::id()),
        key,
    };
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    socket.connect(endpoint).await?;
    register(&socket, &identity).await?;
    // Give the UDP capability one RTT to reach hbbs before opening the TCP request.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let mut tasks = JoinSet::new();
    let heartbeat = socket.clone();
    let heartbeat_id = identity.clone();
    tasks.spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(8)).await;
            advertise(&heartbeat, &heartbeat_id).await?;
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    });
    report("ready", serde_json::json!({"id":identity.id, "role":role}));
    let result = timeout(Duration::from_secs(120), async {
        let session = if role == "listen" {
            let mut buffer = [0u8; p2p::MAX_MESSAGE];
            loop {
                let n = socket.recv(&mut buffer).await?;
                let message = RendezvousMessage::parse_from_bytes(&buffer[..n])?;
                if let Some(rendezvous_message::Union::NativeP2pV1(message)) = message.union {
                    ensure!(message.version == p2p::VERSION, "unknown invitation version");
                    let invite: Invite = protocol::decode(&message.payload, &server)?;
                    break Session::join(endpoint, identity.clone(), server, invite).await?;
                }
            }
        } else {
            Session::begin(endpoint, identity.clone(), server, args.get(4).context("missing target id")?).await?.context("peer does not support native P2P")?
        };
        let started = Instant::now();
        report("ticket", serde_json::json!({"session":session.ticket.session, "initiator":session.ticket.initiator}));
        let stun = std::env::var("NATIVE_P2P_STUN").unwrap_or_else(|_| p2p::STUN_SERVER.into());
        let punching = async {
            if let Ok(address) = std::env::var("NATIVE_P2P_LAB_INTERFACE") {
                let (ip, prefix) = address.split_once('/').context("invalid lab interface")?;
                let interfaces = [p2p::candidates::Interface { address: ip.parse()?, prefix: prefix.parse()? }];
                report("lab_interface_fixture", serde_json::json!(address));
                session.punch_on(&stun, &interfaces).await
            } else {
                session.punch(&stun).await
            }
        };
        let punched = if let Ok(ms) = std::env::var("NATIVE_P2P_CANCEL_MS") {
            timeout(Duration::from_millis(ms.parse()?), punching).await.context("diagnostic user cancellation")??
        } else { punching.await? };
        let deadline = punched.session.deadline;
        let peer = punched.peer;
        let local = punched.socket.local_addr()?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let (kcp, mut stream) = if role == "listen" {
            timeout_at(deadline, kcp_stream::KcpStream::accept(punched.socket.clone(), remaining, None)).await??
        } else {
            timeout_at(deadline, kcp_stream::KcpStream::connect(punched.socket.clone(), remaining)).await??
        };
        let mut proof_identity = identity.clone();
        if std::env::var_os("NATIVE_P2P_BAD_PROOF").is_some() { proof_identity.key = sign::gen_keypair().1; }
        timeout_at(deadline, proof::authenticate(&mut stream, &proof_identity, &punched.session.ticket, &server)).await??;
        let data = randombytes(64 * 1024);
        timeout_at(deadline, async {
            if role == "listen" {
                let received = stream.next_timeout(3000).await.context("KCP data timeout")??;
                ensure!(received.len() == 64 * 1024, "KCP data size mismatch");
                stream.send_bytes(received.freeze()).await?;
                let ack = stream.next_timeout(3000).await.context("KCP acknowledgement timeout")??;
                ensure!(&ack[..] == b"verified", "KCP acknowledgement mismatch");
            } else {
                stream.send_bytes(data.clone().into()).await?;
                let received = stream.next_timeout(3000).await.context("KCP echo timeout")??;
                ensure!(&received[..] == &data[..], "KCP data mismatch");
                stream.send_bytes(hbb_common::bytes::Bytes::from_static(b"verified")).await?;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok::<(), anyhow::Error>(())
        }).await??;
        drop((stream, kcp));
        report("verified", serde_json::json!({"peer":peer,"local":local,"elapsed_ms":started.elapsed().as_millis(),"transport":"authenticated-kcp-diagnostic","strategy":punched.strategy,"history_rewarded":false}));
        Ok::<(), anyhow::Error>(())
    }).await;
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    drop(socket);
    // Let the bounded session-cancellation RPC finish before the runtime exits.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    report("closed", serde_json::json!({"sockets_closed":true}));
    match result {
        Ok(value) => value,
        Err(error) => bail!(error),
    }
}
