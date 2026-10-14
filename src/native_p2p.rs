use base::config::keys;
use hbb_common::{
    anyhow::{Context, Result},
    config::{self, Config},
    log,
    native_p2p::{
        self as p2p,
        protocol::{self, Action, Invite},
        transport::{Identity, Session},
    },
    rendezvous_proto::{NativeP2pV1, RegisterPeer},
    sodiumoxide::crypto::sign,
    tokio::{
        self,
        sync::{OwnedSemaphorePermit, Semaphore},
        time::Instant,
    },
    tokio_util::sync::CancellationToken,
};
use std::sync::{Arc, Mutex, RwLock};

#[cfg(any(target_os = "android", test))]
pub(crate) mod android;
pub(crate) mod waiting;

hbb_common::lazy_static::lazy_static! {
    static ref ATTEMPT: Arc<Semaphore> = Arc::new(Semaphore::new(1));
    static ref PENDING: Mutex<std::collections::HashMap<usize, (usize, CancellationToken)>> = Mutex::new(Default::default());
}

#[derive(Debug)]
pub(crate) struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Native P2P cancelled")
    }
}
impl std::error::Error for Cancelled {}

pub(crate) struct Pending(pub CancellationToken, usize);
impl Pending {
    pub fn new(lc: &Arc<RwLock<crate::client::LoginConfigHandler>>) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let token = CancellationToken::new();
        PENDING.lock().unwrap().insert(id, (Arc::as_ptr(lc) as usize, token.clone()));
        Self(token, id)
    }
}
impl Drop for Pending {
    fn drop(&mut self) { PENDING.lock().unwrap().remove(&self.1); }
}
pub(crate) fn cancel(lc: &Arc<RwLock<crate::client::LoginConfigHandler>>) {
    for (id, token) in PENDING.lock().unwrap().values() {
        if *id == Arc::as_ptr(lc) as usize { token.cancel(); }
    }
}
pub(crate) async fn acquire() -> Option<OwnedSemaphorePermit> {
    ATTEMPT.clone().acquire_owned().await.ok()
}

pub(crate) fn enabled() -> bool {
    #[cfg(target_os = "android")]
    if !android::enabled() {
        return false;
    }
    Config::get_option(keys::OPTION_ENABLE_NATIVE_P2P) != "N"
        && !Config::is_proxy()
        && !config::use_ws()
}

pub(crate) fn identity() -> Result<Identity> {
    p2p::init()?;
    Ok(Identity {
        id: Config::get_id(),
        key: sign::SecretKey::from_slice(&Config::get_key_pair().0)
            .context("native device key unavailable")?,
    })
}

pub(crate) fn slot() -> Option<OwnedSemaphorePermit> {
    ATTEMPT.clone().try_acquire_owned().ok()
}

pub(crate) fn advertise(registration: &mut RegisterPeer) {
    let version = if enabled() { p2p::VERSION } else { 0 };
    let capability = (|| -> Result<_> {
        let identity = identity()?;
        protocol::encode(
            &protocol::request(
                &identity.id,
                Action::Advertise {
                    capability: p2p::CAPABILITY.into(),
                    enabled: version > 0,
                },
            )?,
            &identity.key,
        )
    })();
    match capability {
        Ok(payload) => {
            registration.native_p2p_version = version;
            registration.native_p2p_capability = payload.into();
        }
        Err(error) => log::trace!("native capability unavailable: {error}"),
    }
}

pub(crate) fn invite(message: NativeP2pV1, endpoint: String, server: crate::server::ServerPtr) {
    if message.version != p2p::VERSION {
        return;
    }
    if !enabled() {
        #[cfg(target_os = "android")]
        android::decline(message, endpoint);
        return;
    }
    let Some(slot) = slot() else {
        return;
    };
    tokio::spawn(async move {
        if let Err(error) = accept(message, &endpoint, server, slot).await {
            log::trace!("native-p2p accept ended: {error:#}");
        }
    });
}

async fn accept(
    message: NativeP2pV1,
    endpoint: &str,
    server: crate::server::ServerPtr,
    slot: OwnedSemaphorePermit,
) -> Result<()> {
    let key =
        crate::get_rs_pk(&crate::get_key(true).await).context("native server key unavailable")?;
    let invite: Invite = protocol::decode(&message.payload, &key)?;
    let mut session = Session::join(endpoint, identity()?, key, invite).await?;
    #[cfg(not(target_os = "android"))]
    let interfaces = p2p::interfaces::physical().unwrap_or_default();
    #[cfg(target_os = "android")]
    let environment = android::environment().context("Android native P2P environment unavailable")?;
    #[cfg(target_os = "android")]
    let interfaces = environment.interfaces.clone();
    let accepted = async {
        let (socket, peer, _) = tokio::select! {
            result = session.probe(p2p::STUN_SERVER, &interfaces, None) => result?,
            _ = async { while enabled() { tokio::time::sleep(std::time::Duration::from_millis(250)).await; } } => return Err(Cancelled.into()),
        };
        let deadline = session.deadline;
        let cancellation = async {
            tokio::select! {
                _ = session.wait_for_cancellation() => {},
                _ = async { while enabled() { tokio::time::sleep(std::time::Duration::from_millis(250)).await; } } => {},
            }
        };
        let (kcp, stream) = crate::kcp_stream::KcpStream::native(socket,
            deadline.saturating_duration_since(Instant::now()), true, cancellation).await?;
        Ok::<_, hbb_common::anyhow::Error>((kcp, stream, peer))
    }.await;
    let (kcp, stream, peer) = match accepted {
        Ok(result) => result,
        Err(error) => { session.close().await; return Err(error); }
    };
    log::info!("native-p2p KCP accepted from {peer}");
    session.handoff_to_caller();
    drop(session);
    drop(slot);
    // Preserve the existing identity, password/confirmation, whitelist and permission checks.
    let result = crate::server::create_tcp_connection(
        server,
        stream,
        peer,
        true,
        crate::server::ConnectionMeta::default(),
    )
    .await;
    kcp.close_native().await;
    result
}
