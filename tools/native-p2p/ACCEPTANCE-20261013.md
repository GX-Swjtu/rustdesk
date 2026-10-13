# Native P2P v4 acceptance: 2026-10-13

All times below use Asia/Shanghai. This record covers the adaptive traversal
extension and the supplementary test after the interactive desktop was restored.
Both Windows machines retained Mihomo TUN throughout the recorded connections.

## Tested binaries and source

| Component | Runtime build revision |
| --- | --- |
| Windows client | `02c7a5331370cb4510d1e0de7c5eeccd552d7686` |
| Client common baseline | `de8230b6d3ccf94946c0eeb54c53c56b26352b73` |
| Test hbbs | `bc6ba11d15803944cd8ee54961c042c63fd6cdb5` |
| Server common baseline | `b444afa824b723cfef62cf954fa9a4e0fe52680f` |

The subsequent delivery pins (`33107c670`, `428a7e8`, `988a865`, `281a0d3`)
only add a parallel STUN test-fixture correction; runtime code is unchanged.
The client DLL SHA-256 is
`d5faf89e9bb540bf4ae8a5e9e224dc8b240b486cc56f14db05b02bda5055b73f`.
Production RustDesk and frp services were not replaced by the isolated test server.

## Actual Windows remote desktop

Native success here requires authenticated KCP, login/PeerInfo and video-reception
evidence. A UDP probe reply alone is not counted as remote-desktop success.

| Group | Native desktop | Relay desktop | Other |
| --- | --- | --- | --- |
| Home to school, 17:52-17:55 | 10/10 | 0 | 0 |
| School to home, first group, 18:00-18:06 | 8/10 | 1 | 1 early exit after authentication/login |
| School to home, 18:07 supplement | 1/1 | 0 | 0 |
| School to home, fixed 10-trial recheck, 18:11-18:16 | 9/10 | 1 | 0 |
| School to home, restored desktop, 20:14-20:17 | 1/1 | 0 | 0 |

The original first-group 8/10 result is retained. The recheck and supplementary
connections do not replace it or establish a long-term success rate. All of
these connections selected Mode 2 / TTL 7.

At 18:00:54, one session authenticated and received PeerInfo, then exited its IO
loop before video-reception evidence. The user later suspected that leaving the
desktop caused the interruption; this is a possible explanation, not a confirmed
cause. The restored-desktop supplement authenticated at 20:14:57, recorded video
reception at 20:15:00, and remained connected for 133 seconds before the harness
closed it. Nine samples showed one caller UDP socket, zero test-relay TCP
connections and TUN Up. The server capture, started before the caller, recorded
zero packets on the test relay port and zero kernel drops.

The test caller and capture exited. Both services remained running, and their
server children returned to the two ordinary UDP sockets present before the test.
The original 31-trial summary remains unchanged; the restored-desktop connection
is recorded separately. This short hold does not replace a 30-minute soak test.

## Component checks and limits

- Windows and Linux common-module tests: 24/24 each. The final Linux tree was
  rerun during review and passed all 24 tests.
- Windows client integration tests: 2/2, covering disabled-path skipping and late
  native probe replies at the KCP handoff.
- Admission/signature/withdrawal checks passed. Mixed v3/v4 diagnostics succeeded
  in both directions without negotiating the extension; an older native server
  also retained the original strategy.
- Windows/max diagnostics: Mode 2 succeeded five times across TTL 7, TTL 4 and OS
  TTL; Mode 1 timed out once within its bounded window. These diagnostics preceded
  the final reflected-primer fix. There is no successful cross-network Mode 1
  sample yet; its loopback tests do not establish real-network effectiveness.
- Android v4 validation checked that the target dependency graph excludes the
  new module. The earlier arm64 build/APK and v2 soak, input, clipboard and 100 MiB
  transfer results are separate evidence. They were not rerun as v4 acceptance.
  Android real-device interoperability remains deferred at the user's request.

## Remaining frp strategy coverage

Reference: frp v0.71.0, commit `4a23aa181c1d7e28eecaa8216024ed753b9d27c8`.
The fork implements Mode 1 with a fixed two-second sender delay and Mode 2,
each with TTL 7, TTL 4 and OS TTL, plus bounded strategy scoring.

| Not introduced | Purpose in frp |
| --- | --- |
| Mode 0 | Simple probing, principally for two EasyNATs or a public-network receiver; includes role and delay variants. |
| Mode 1 immediate-send variants | Try sending without the two-second delay for each of the three TTL choices. |
| Mode 3 | Predict both peers' ports when both HardNAT mappings change regularly. |
| Mode 4 | Combine prediction with multiple listening sockets and random destination ports when only one HardNAT is predictable. |
| Assisted local addresses | Add local-interface candidates alongside mapped public addresses. |
| Changed public-IP candidates | Classify and consider NAT observations whose public IP changes; this fork currently rejects inconsistent exits. |

frp's persistent XTCP tunnel, bounded background retries and reuse by later
connections are also not introduced. These are tunnel-lifecycle features rather
than additional hole-punching modes. The existing RustDesk fallback remains in use.

Source comparisons: [mode/TTL/delay choices](https://github.com/fatedier/frp/blob/v0.71.0/pkg/nathole/analysis.go),
[probe modes and assisted addresses](https://github.com/fatedier/frp/blob/v0.71.0/pkg/nathole/nathole.go),
[NAT classification](https://github.com/fatedier/frp/blob/v0.71.0/pkg/nathole/classify.go),
and [XTCP lifecycle](https://github.com/fatedier/frp/blob/v0.71.0/client/visitor/xtcp.go).

## Evidence archive

Detailed logs, packet captures, packages and manifests remain in the workspace's
`artifacts/strategies-20261013/` directory, outside the source repositories.
Relevant entries are `windows-v4-cold-summary.json`, `build-manifest.json`, the
`relay-correlation-*.json` files, and
`restored-desktop-20261013-201208/supplement-summary.json` with its raw trial log,
capture statistics and endpoint cleanup records. This source-controlled summary
does not include credentials, raw packet captures or machine configuration files.
