use hbb_common::{anyhow::{ensure, Context, Result}, native_p2p::{protocol::{self, Ticket}, transport::Identity},
    protobuf::Message, sodiumoxide::{crypto::sign, randombytes::randombytes}, Stream};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    context: String,
    session: String,
    id: String,
    nonce: Vec<u8>,
    echo: Vec<u8>,
}

// This is diagnostic authentication, not a substitute for RustDesk's login/permissions.
pub async fn authenticate(stream: &mut Stream, identity: &Identity, ticket: &Ticket, server: &sign::PublicKey) -> Result<()> {
    let peer_key = sign::verify(&ticket.signed_peer, server).map_err(|_| hbb_common::anyhow::anyhow!("invalid peer certificate"))?;
    let peer_key = hbb_common::rendezvous_proto::IdPk::parse_from_bytes(&peer_key)?;
    ensure!(peer_key.id == ticket.peer_id, "diagnostic peer ID mismatch");
    let pk = sign::PublicKey::from_slice(&peer_key.pk).context("invalid diagnostic peer key")?;
    let nonce = randombytes(32);
    let mut proof = Proof { context: "native-udp-kcp-diagnostic".into(), session: ticket.session.clone(), id: identity.id.clone(), nonce: nonce.clone(), echo: vec![] };
    for round in 0..2 {
        stream.send_bytes(protocol::encode(&proof, &identity.key)?.into()).await?;
        let bytes = stream.next_timeout(3000).await.context("diagnostic proof timeout")??;
        let peer: Proof = protocol::decode(&bytes, &pk)?;
        ensure!(peer.context == proof.context && peer.session == ticket.session && peer.id == ticket.peer_id && peer.nonce.len() == 32, "diagnostic transcript mismatch");
        if round == 1 { ensure!(peer.echo == nonce, "diagnostic challenge mismatch"); }
        proof.echo = peer.nonce;
    }
    Ok(())
}
