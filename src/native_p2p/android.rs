//! Android-only registration and optional LAN discovery; no activity/network gate.
use super::*;
use hbb_common::{native_p2p::candidates::Interface, serde_derive::Deserialize};
use std::{future::Future, net::Ipv4Addr};

#[path = "android_registration.rs"]
mod registration;
#[cfg(target_os = "android")]
#[path = "android_interfaces.rs"]
mod interfaces;
#[cfg(target_os = "android")]
pub(crate) use registration::{begin_controlled, record as record_registration};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Address {
    address: Ipv4Addr,
    prefix: u8,
}

fn parse_interfaces(value: &str) -> Result<Vec<Interface>> {
    hbb_common::anyhow::ensure!(value.len() <= 4096, "Android LAN candidates too large");
    let addresses: Vec<Address> = hbb_common::serde_json::from_str(value)?;
    hbb_common::anyhow::ensure!(addresses.len() <= p2p::MAX_LOCAL, "too many Android LAN candidates");
    hbb_common::anyhow::ensure!(addresses.iter().all(|a| (1..=32).contains(&a.prefix)
        && !a.address.is_unspecified() && !a.address.is_loopback()
        && !a.address.is_link_local() && !a.address.is_multicast()
        && !a.address.is_broadcast() && a.address.octets()[0] != 0
        && a.address.octets()[0] < 240), "invalid Android LAN candidate");
    Ok(addresses.into_iter().map(|a| Interface { address: a.address, prefix: a.prefix }).collect())
}

#[derive(Default)]
struct State {
    cancelled: CancellationToken,
    revision: u64,
}

impl State {
    fn settings_changed(&mut self) {
        self.cancelled.cancel();
        self.cancelled = CancellationToken::new();
        self.revision = self.revision.wrapping_add(1);
    }

    fn environment(&self, option: &str, disable_udp: &str) -> Option<Environment> {
        if option == "N" || disable_udp == "Y" { return None; }
        Some(Environment { interfaces: Vec::new(), cancelled: self.cancelled.clone() })
    }
}

hbb_common::lazy_static::lazy_static! {
    static ref SETTINGS: Mutex<State> = Mutex::new(State::default());
}

pub(crate) struct Environment {
    pub interfaces: Vec<Interface>,
    cancelled: CancellationToken,
}

impl Environment {
    pub async fn run<T>(&self, operation: impl Future<Output = Result<T>>) -> Result<T> {
        tokio::select! {
            biased;
            _ = self.cancelled.cancelled() => hbb_common::anyhow::bail!("Android native P2P settings changed"),
            result = operation => result,
        }
    }
}

pub(crate) fn environment() -> Option<Environment> {
    let environment = SETTINGS.lock().unwrap().environment(
        &Config::get_option(keys::OPTION_ENABLE_NATIVE_P2P),
        &Config::get_option(keys::OPTION_DISABLE_UDP),
    );
    #[cfg(target_os = "android")]
    let environment = environment.map(|mut environment| {
        match interfaces::read().and_then(|value| parse_interfaces(&value)) {
            Ok(interfaces) => environment.interfaces = interfaces,
            Err(error) => log::debug!("Android native P2P LAN candidates unavailable: {error}"),
        }
        environment
    });
    environment
}

pub(crate) fn enabled() -> bool {
    Config::get_option(keys::OPTION_ENABLE_NATIVE_P2P) != "N"
        && Config::get_option(keys::OPTION_DISABLE_UDP) != "Y"
}

pub(crate) fn registration_changed(previous: &mut u64) -> bool {
    let revision = SETTINGS.lock().unwrap().revision;
    let changed = *previous != revision;
    *previous = revision;
    changed
}

pub(crate) fn settings_changed() { SETTINGS.lock().unwrap().settings_changed(); }

pub(super) fn refresh_registration() {
    let mut state = SETTINGS.lock().unwrap();
    state.revision = state.revision.wrapping_add(1);
}

// A just-withdrawn UDP registration can race with an invitation. Cancel its
// authenticated lease so the caller need not spend a full probe window waiting.
#[cfg(target_os = "android")]
pub(crate) fn decline(message: NativeP2pV1, endpoint: String) {
    let Some(slot) = super::slot() else { return; };
    tokio::spawn(async move {
        let _slot = slot;
        let result = async {
            let key = crate::get_rs_pk(&crate::get_key(true).await)
                .context("native server key unavailable")?;
            let invite: Invite = protocol::decode(&message.payload, &key)?;
            let identity = super::identity()?;
            hbb_common::anyhow::ensure!(invite.target == identity.id && invite.expires > protocol::now()?, "invalid native invitation");
            p2p::transport::rpc(&endpoint, &identity, &key,
                Action::Cancel { session: invite.session }, std::time::Duration::from_secs(1)).await
        }.await;
        if let Err(error) = result { log::trace!("Android native P2P decline: {error}"); }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_defaults_on_without_activity_wifi_or_lan_candidates() {
        let state = State::default();
        for option in ["", "Y"] {
            let environment = state.environment(option, "").unwrap();
            assert!(environment.interfaces.is_empty());
        }
        assert!(state.environment("N", "").is_none());
        assert!(state.environment("", "Y").is_none());
        assert!(parse_interfaces("[]").unwrap().is_empty());
    }

    #[tokio::test]
    async fn disabling_native_settings_cancels_and_releases_probe_socket() {
        let mut state = State::default();
        let environment = state.environment("", "").unwrap();
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let operation = async move {
            let _socket = socket;
            std::future::pending::<Result<()>>().await
        };
        state.settings_changed();
        assert!(environment.run(operation).await.is_err());
        assert!(tokio::net::UdpSocket::bind(address).await.is_ok());
    }

    #[test]
    fn lan_candidates_remain_bounded_and_on_link() {
        let interfaces = parse_interfaces(r#"[{"address":"192.168.2.10","prefix":24}]"#).unwrap();
        assert!(interfaces[0].on_link("192.168.2.20:4000".parse().unwrap()));
        assert!(!interfaces[0].on_link("192.168.3.20:4000".parse().unwrap()));
        assert!(parse_interfaces(r#"[{"address":"169.254.2.10","prefix":24}]"#).is_err());
        let too_many = format!("[{}]", vec![r#"{"address":"192.168.2.10","prefix":24}"#; p2p::MAX_LOCAL + 1].join(","));
        assert!(parse_interfaces(&too_many).is_err());
    }
}
