//! One window owns its policy; each pending transport owns an independent flow.
//! Neither this policy nor diagnostic data is written to peer preferences.
use hbb_common::{native_p2p::{self as p2p, progress::{Observer, Progress, Reason}},
    tokio::time::Instant, tokio_util::sync::CancellationToken};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::{BTreeMap, VecDeque}, sync::{Arc, Mutex}, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice { Pending, Direct, Relay, Cancel, Legacy }

#[derive(Default)]
pub struct Context(Mutex<Window>);
#[derive(Default)]
struct Window {
    closed: bool,
    relay: bool,
    next: u64,
    mapping_generation: u64,
    mappings: BTreeMap<i32, u64>,
    flows: BTreeMap<u64, Arc<Flow>>,
}
#[derive(Clone)]
pub struct Scope { pub window: Arc<Context>, pub mapping: Option<(i32, u64)> }

pub struct Flow {
    pub id: u64,
    pub cancel: CancellationToken,
    started: Instant,
    inner: Mutex<State>,
}
struct State {
    choice: Choice,
    round_start: Instant,
    retry_at: Option<Instant>,
    snapshot: Snapshot,
}
#[derive(Clone, Serialize)]
pub struct Snapshot {
    id: String,
    mapping: Option<i32>,
    round: u64,
    stage: String,
    elapsed_ms: u64,
    round_ms: u64,
    limit_ms: u64,
    retry_ms: Option<u64>,
    reason: Option<String>,
    local: Value,
    peer: Value,
    strategy: Value,
    probes: Value,
    rounds: VecDeque<Value>,
    events: VecDeque<Value>,
    active: bool,
}

fn bounded_push(queue: &mut VecDeque<Value>, value: Value, limit: usize) {
    if queue.len() == limit { queue.pop_front(); }
    queue.push_back(value);
}
fn network(candidates: &p2p::candidates::Candidates) -> Value {
    json!({ "candidates": candidates, "profile": p2p::strategy::classify(candidates).ok(),
        "prediction_evidence": p2p::strategy::classify(candidates).map(|p| p.predictable).unwrap_or(false) })
}
pub fn backoff(round: u64) -> Duration {
    Duration::from_secs(match round { 0 | 1 => 2, 2 => 5, _ => 10 })
}

/// Unknown transport failures are retryable. Only explicit evidence stops probes.
pub fn terminal_reason(error: &str) -> bool {
    if let Some(reason) = Reason::decode(error) { return matches!(reason.code.as_str(), "no_strategy" | "unsupported"); }
    let error = error.to_ascii_lowercase();
    ["no supported native udp nat combination", "no eligible wan strategy", "no applicable strategy",
        "prediction lacks evidence", "coordinator unsupported", "unsupported native version",
        "capability not negotiated", "identity mismatch", "identity handshake failed", "invalid signature",
        "signature verification", "signature mismatch", "native signature encoding", "invalid sealed probe key", "invalid native ticket", "invalid native plan",
        "native server key unavailable", "native server key mismatch", "native device key unavailable",
        "wrong their public length", "invalid probe key", "unknown native version", "native response replay"]
        .iter().any(|part| error.contains(part))
}

impl Context {
    pub fn mapping(&self, port: i32) -> (i32, u64) {
        let mut window = self.0.lock().unwrap();
        window.mapping_generation += 1;
        let generation = window.mapping_generation;
        window.mappings.insert(port, generation);
        (port, generation)
    }

    pub fn begin(&self, mapping: Option<(i32, u64)>) -> Arc<Flow> {
        let mut window = self.0.lock().unwrap();
        window.next += 1;
        // Keep only recent completed flows. Active mappings remain independent.
        let finished: Vec<_> = window.flows.iter().filter(|(_, f)| !f.snapshot().active).map(|(id, _)| *id).collect();
        for id in finished.iter().take(finished.len().saturating_sub(19)) { window.flows.remove(id); }
        let flow = Arc::new(Flow::new(window.next, mapping.map(|m| m.0)));
        let removed = mapping.is_some_and(|(port, generation)| window.mappings.get(&port) != Some(&generation));
        if window.closed || removed { flow.choose(Choice::Cancel); }
        else if window.relay { flow.choose(Choice::Relay); }
        window.flows.insert(flow.id, flow.clone());
        flow
    }

    pub fn relay(&self, id: u64) -> bool {
        let mut window = self.0.lock().unwrap();
        if window.closed || !window.flows.get(&id).is_some_and(|f| f.choice() == Choice::Pending) { return false; }
        window.relay = true;
        for flow in window.flows.values() { flow.choose(Choice::Relay); }
        true
    }

    pub fn commit_direct(&self, flow: &Flow) -> bool {
        // Same lock/order as relay/close: only one outcome can win.
        let window = self.0.lock().unwrap();
        if window.closed || window.relay { return false; }
        flow.choose(Choice::Direct)
    }

    pub fn finish(&self, flow: &Flow) -> Choice {
        // Serialize handoff with window/mapping cancellation after round cleanup.
        let _window = self.0.lock().unwrap();
        let mut state = flow.inner.lock().unwrap();
        state.snapshot.active = false;
        state.choice
    }

    pub fn cancel_flow(&self, id: u64) -> bool {
        self.0.lock().unwrap().flows.get(&id).is_some_and(|flow| flow.choose(Choice::Cancel))
    }

    pub fn cancel_mapping(&self, mapping: i32) {
        let mut window = self.0.lock().unwrap();
        window.mappings.remove(&mapping);
        for flow in window.flows.values() {
            if flow.snapshot().mapping == Some(mapping) { flow.choose(Choice::Cancel); }
        }
    }

    pub fn close(&self) {
        let mut window = self.0.lock().unwrap();
        window.closed = true;
        for flow in window.flows.values() { flow.choose(Choice::Cancel); }
    }

    pub fn snapshots(&self) -> String {
        let snapshots: Vec<_> = self.0.lock().unwrap().flows.values().map(|f| f.snapshot()).collect();
        serde_json::to_string(&snapshots).unwrap_or_else(|_| "[]".into())
    }
}

impl Flow {
    fn new(id: u64, mapping: Option<i32>) -> Self {
        let now = Instant::now();
        Self { id, cancel: CancellationToken::new(), started: now, inner: Mutex::new(State {
            choice: Choice::Pending, round_start: now, retry_at: None,
            snapshot: Snapshot { id: id.to_string(), mapping, round: 0, stage: "slot".into(), elapsed_ms: 0,
                round_ms: 0, limit_ms: p2p::ATTEMPT_SECONDS * 1000, retry_ms: None, reason: None,
                local: Value::Null, peer: Value::Null, strategy: Value::Null, probes: Value::Null,
                rounds: VecDeque::new(), events: VecDeque::new(), active: true },
        }) }
    }
    pub fn legacy(&self) { self.choose(Choice::Legacy); }
    pub fn stage(&self, stage: &'static str) {
        let generation = self.inner.lock().unwrap().snapshot.round;
        self.progress(generation, Progress::Stage(stage));
    }
    pub fn choice(&self) -> Choice { self.inner.lock().unwrap().choice }
    fn choose(&self, choice: Choice) -> bool {
        let mut state = self.inner.lock().unwrap();
        let stop_fallback = choice == Choice::Cancel && matches!(state.choice, Choice::Relay | Choice::Legacy);
        if !state.snapshot.active || (state.choice != Choice::Pending && !stop_fallback) { return false; }
        state.choice = choice;
        let generation = state.snapshot.round;
        bounded_push(&mut state.snapshot.events, json!({"ms": self.started.elapsed().as_millis() as u64,
            "round": generation, "choice": format!("{choice:?}")}), 200);
        if choice != Choice::Direct {
            state.snapshot.stage = if choice == Choice::Relay { "relay" } else { "cancelling" }.into();
            self.cancel.cancel();
        }
        true
    }
    pub fn start_round(self: &Arc<Self>) -> (u64, Observer) {
        let round = {
            let mut state = self.inner.lock().unwrap();
            state.snapshot.round += 1;
            state.round_start = Instant::now();
            state.retry_at = None;
            state.snapshot.local = Value::Null;
            state.snapshot.peer = Value::Null;
            state.snapshot.strategy = Value::Null;
            state.snapshot.probes = Value::Null;
            state.snapshot.limit_ms = p2p::ATTEMPT_SECONDS * 1000;
            state.snapshot.round
        };
        let flow = self.clone();
        (round, Some(Arc::new(move |progress| flow.progress(round, progress))))
    }
    pub fn limit(&self, deadline: Instant) {
        let mut state = self.inner.lock().unwrap();
        state.snapshot.limit_ms = deadline.saturating_duration_since(state.round_start).as_millis() as u64;
    }
    fn progress(&self, generation: u64, progress: Progress) {
        let mut state = self.inner.lock().unwrap();
        if state.choice != Choice::Pending || state.snapshot.round != generation { return; }
        match progress {
            Progress::Stage(stage) => {
                state.snapshot.stage = stage.into();
                bounded_push(&mut state.snapshot.events, json!({"ms": self.started.elapsed().as_millis() as u64, "round": generation, "stage": stage}), 200);
            }
            Progress::Local(candidates) => state.snapshot.local = network(&candidates),
            Progress::Selected(plan) => {
                state.snapshot.peer = network(&plan.peer);
                state.snapshot.strategy = json!({"behavior": plan.behavior, "sender": plan.sender, "lan_only": plan.lan_only});
            }
            Progress::Probes { sent, authenticated, sockets } => state.snapshot.probes = json!({"sent": sent, "authenticated": authenticated, "sockets": sockets}),
        }
    }
    pub fn finish_round(&self, reason: &str, parked: bool) {
        let mut state = self.inner.lock().unwrap();
        let safe: String = reason.chars().take(3072).collect();
        if let Some(reason) = Reason::decode(&safe) {
            if let Some(local) = reason.local { state.snapshot.local = network(&local); }
            if let Some(peer) = reason.peer { state.snapshot.peer = network(&peer); }
            state.snapshot.reason = Some(reason.message);
        } else { state.snapshot.reason = Some(safe); }
        let summary = json!({"round": state.snapshot.round, "elapsed_ms": state.round_start.elapsed().as_millis() as u64,
            "reason": state.snapshot.reason, "strategy": state.snapshot.strategy, "probes": state.snapshot.probes});
        bounded_push(&mut state.snapshot.rounds, summary, 20);
        if state.choice == Choice::Pending {
            state.snapshot.stage = if parked { "waiting" } else { "backoff" }.into();
            state.retry_at = if parked { None } else { Some(Instant::now() + backoff(state.snapshot.round)) };
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        let state = self.inner.lock().unwrap();
        let mut snapshot = state.snapshot.clone();
        snapshot.elapsed_ms = self.started.elapsed().as_millis() as u64;
        snapshot.round_ms = state.round_start.elapsed().as_millis() as u64;
        snapshot.retry_ms = state.retry_at.map(|at| at.saturating_duration_since(Instant::now()).as_millis() as u64);
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_results_and_old_generations_cannot_change_a_new_round_or_window() {
        let window = Context::default();
        let other = Context::default();
        let old_mapping = window.mapping(1001);
        let a = window.begin(Some(old_mapping));
        let b = window.begin(Some(window.mapping(1002)));
        let c = other.begin(None);
        let (_, old) = a.start_round();
        let (_, current) = a.start_round();
        p2p::progress::report(&current, Progress::Stage("secure"));
        p2p::progress::report(&old, Progress::Stage("network"));
        assert_eq!(a.snapshot().stage, "secure");
        window.cancel_mapping(1001);
        assert_eq!(a.choice(), Choice::Cancel);
        assert_eq!(window.begin(Some(old_mapping)).choice(), Choice::Cancel);
        let new_mapping = window.mapping(1001);
        assert_eq!(window.begin(Some(old_mapping)).choice(), Choice::Cancel);
        assert_eq!(window.begin(Some(new_mapping)).choice(), Choice::Pending);
        assert_eq!(b.choice(), Choice::Pending);
        assert!(window.relay(b.id));
        assert!(!window.relay(b.id));
        assert!(!window.commit_direct(&b));
        assert_eq!(window.begin(None).choice(), Choice::Relay);
        assert_eq!(c.choice(), Choice::Pending);
        assert!(other.commit_direct(&c));
        assert!(!other.relay(c.id));
        assert!(!other.cancel_flow(c.id));
        other.close();
        assert_eq!(other.begin(None).choice(), Choice::Cancel);
        assert_eq!(Context::default().begin(None).choice(), Choice::Pending);
    }

    #[test]
    fn thousand_rounds_keep_only_bounded_secret_free_diagnostics() {
        let window = Context::default();
        let flow = window.begin(None);
        for round in 1..=1000 {
            let (generation, observer) = flow.start_round();
            assert_eq!(generation, round);
            for stage in ["prepare", "network", "peer", "probe", "kcp", "secure"] {
                p2p::progress::report(&observer, Progress::Stage(stage));
            }
            flow.finish_round("network timeout; peer capabilities unknown", false);
        }
        let snapshot = flow.snapshot();
        assert_eq!(snapshot.rounds.len(), 20);
        assert_eq!(snapshot.events.len(), 200);
        assert_eq!(snapshot.rounds.front().unwrap()["round"], 981);
        assert!(window.snapshots().len() < 40000);
        for (round, seconds) in [(1, 2), (2, 5), (3, 10), (1000, 10)] {
            assert_eq!(backoff(round), Duration::from_secs(seconds));
        }
        for transient in ["deadline has elapsed", "native device busy", "STUN timed out", "peer cancelled or session expired"] {
            assert!(!terminal_reason(transient));
        }
        for terminal in ["native signature mismatch", "Signature mismatch", "Wrong their public length",
            "native server key mismatch", "native target identity mismatch", "native UDP has no eligible WAN strategy after LAN probe"] {
            assert!(terminal_reason(terminal));
        }
    }

    #[test]
    fn relay_cancel_and_success_are_arbitrated_in_every_phase() {
        for stage in ["slot", "prepare", "network", "peer", "probe", "kcp", "secure", "backoff", "waiting"] {
            for first in [Choice::Direct, Choice::Relay, Choice::Cancel] {
                let window = Context::default();
                let flow = window.begin(None);
                flow.stage(stage);
                match first {
                    Choice::Direct => assert!(window.commit_direct(&flow)),
                    Choice::Relay => assert!(window.relay(flow.id)),
                    _ => assert!(window.cancel_flow(flow.id)),
                }
                assert!(!window.relay(flow.id));
                assert!(!window.commit_direct(&flow));
                assert_eq!(window.finish(&flow), first);
                assert!(!window.cancel_flow(flow.id));
                assert_eq!(flow.choice(), first);
            }
        }
    }
}
