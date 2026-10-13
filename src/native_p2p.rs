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
        time::{timeout_at, Instant},
    },
    tokio_util::sync::CancellationToken,
};
use std::sync::{Arc, Mutex, RwLock};

hbb_common::lazy_static::lazy_static! {
    static ref ATTEMPT: Arc<Semaphore> = Arc::new(Semaphore::new(1));
    static ref PENDING: Mutex<Option<(usize, CancellationToken)>> = Mutex::new(None);
}

#[derive(Debug)]
pub(crate) struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Native P2P cancelled")
    }
}
impl std::error::Error for Cancelled {}

pub(crate) struct Pending(pub CancellationToken);

impl Pending {
    pub fn new(lc: &Arc<RwLock<crate::client::LoginConfigHandler>>) -> Self {
        let token = CancellationToken::new();
        *PENDING.lock().unwrap() = Some((Arc::as_ptr(lc) as usize, token.clone()));
        Self(token)
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        PENDING.lock().unwrap().take();
    }
}

pub(crate) fn cancel(lc: &Arc<RwLock<crate::client::LoginConfigHandler>>) {
    if let Some((id, token)) = PENDING.lock().unwrap().as_ref() {
        if *id == Arc::as_ptr(lc) as usize {
            token.cancel();
        }
    }
}

pub(crate) fn enabled() -> bool {
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
    if !enabled() || message.version != p2p::VERSION {
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
    let session = Session::join(endpoint, identity()?, key, invite).await?;
    let mut punched = session.punch(p2p::STUN_SERVER).await?;
    let deadline = punched.session.deadline;
    let peer = punched.peer;
    let (kcp, stream) = timeout_at(
        deadline,
        crate::kcp_stream::KcpStream::accept(
            punched.socket.clone(),
            deadline.saturating_duration_since(Instant::now()),
            None,
        ),
    )
    .await??;
    log::info!("native-p2p KCP accepted from {peer}");
    punched.session.handoff_to_caller();
    drop(punched);
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
    drop(kcp);
    result
}
