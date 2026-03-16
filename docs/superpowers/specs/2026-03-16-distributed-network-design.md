# LMB Distributed P2P Network Design

## Overview

Transform LMB from a standalone Luau runtime into a distributed P2P network where nodes can collaborate — managing the network, persisting data with replication, and dispatching work across the cluster.

### Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Network topology | P2P (no central server) | Resilience, no single point of failure |
| Transport | libp2p | Mature Rust ecosystem, built-in NAT traversal, mDNS, gossipsub |
| Authorization | CA certificate model | Strongest security for permissioned networks |
| Role system | All roles enabled by default, can disable | Low barrier to entry for newcomers |
| Data replication | Raft consensus (openraft) | Strong consistency across Storage nodes |
| Management node | Single initially, architecture ready for multi | Simplicity first, extensibility later |
| CRL distribution | Forced sync on join + gossip propagation | Covers both online and offline rejoining scenarios |
| Node identification | Short ID (8 chars) + optional alias | Balance between usability and uniqueness |
| POSIX compliance | CLI, XDG paths, signal handling | Standards-based, predictable behavior |

### Delivery Phases

| Phase | Scope | Dependencies |
|-------|-------|--------------|
| Phase 1 | Network foundation + CA authentication | None |
| Phase 2 | Role system + work dispatch | Phase 1 |
| Phase 3 | Distributed Store (Raft replication) | Phase 1 |
| Phase 4 | Operations tooling + monitoring | Phase 1-3 |

Phase 2 and Phase 3 can be developed in parallel after Phase 1 is complete.

### Backward Compatibility

When network features are not enabled (no `config.toml`, no `lmb network start`), the existing `lmb eval` and `lmb serve` commands work exactly as before with zero behavior change. The network module is feature-gated at compile time (`--features network`) and opt-in at runtime (only activates when `lmb network start` is called or network configuration is present).

---

## POSIX Compliance

All phases adhere to the following POSIX conventions.

### CLI Conventions

- Short flags (`-v`) and long flags (`--verbose`) following GNU style
- `--` separator support for all commands
- Exit codes: `0` success, `1` general error, `2` usage error
- Errors and diagnostics to stderr, normal output to stdout
- Long-running operations emit progress to stderr

### XDG Base Directory

```
$XDG_CONFIG_HOME/lmb/             # Default: ~/.config/lmb/
└── config.toml                    # Node configuration (roles, listen address, bootstrap nodes)

$XDG_DATA_HOME/lmb/               # Default: ~/.local/share/lmb/
├── identity.key                   # Ed25519 private key (loss = loss of node identity)
├── node.crt                       # CA-signed node certificate
├── ca.crt                         # CA root certificate (received when joining network)
├── ca.key                         # CA private key (manager nodes only)
└── store.db                       # SQLite default storage location

$XDG_STATE_HOME/lmb/              # Default: ~/.local/state/lmb/
├── crl.bin                        # Locally cached CRL (versioned + signed)
└── peers.json                     # Known peers cache (last-seen peer list)
```

**Data vs State distinction:**

- **Config** (`$XDG_CONFIG_HOME`) — User-editable configuration. Can be recreated by the user or regenerated with defaults. Example: `config.toml`.
- **Data** (`$XDG_DATA_HOME`) — Persistent, should be backed up. Loss means re-joining the network or losing user data. Examples: identity key (loss = loss of node identity), certificates, store database.
- **State** (`$XDG_STATE_HOME`) — Rebuildable, no backup needed. Loss is merely inconvenient — the data can be re-fetched or re-discovered automatically. Examples: CRL cache (re-fetched from manager), peers cache (re-discovered via mDNS/bootstrap).

All paths can be overridden via environment variables:

| Variable | Purpose | Corresponding flag |
|----------|---------|-------------------|
| `LMB_CONFIG_DIR` | Config directory | `--config-dir` |
| `LMB_DATA_DIR` | Data directory | `--data-dir` |
| `LMB_STATE_DIR` | State directory | `--state-dir` |

If XDG variables are not set, falls back to `$HOME/.config/lmb/`, `$HOME/.local/share/lmb/`, `$HOME/.local/state/lmb/`.

### Signal Handling

| Signal | Behavior |
|--------|----------|
| **SIGINT** (Ctrl+C) | Graceful shutdown: stop accepting new connections, wait for in-progress work (max 30s), notify connected peers, disconnect, exit |
| **SIGTERM** | Same as SIGINT |
| **SIGHUP** | Reload `config.toml` without dropping connections (role changes, listen address, etc.) |
| **Second SIGINT** | Force exit (do not wait for in-progress operations) |

### Node Identification

Nodes are identified by libp2p `PeerId` (Ed25519-derived, Base58-encoded, 52 characters). For usability:

- **Display**: Show alias if set, otherwise show short ID (first 8 characters of PeerId)
- **CLI input**: Accept alias, short ID, or full PeerId
- **Alias**: Optional, configured via `[node] name = "worker-01"` in config.toml, broadcast with role announcements
- **Collision handling**: If a short ID is ambiguous, prompt user to provide more characters

```
$ lmb network peers
NAME         SHORT ID      ROLES                STATUS
worker-01    12D3KooW      worker               connected
storage-a    7Qp2YmRk      storage              connected
             Bx9fTn3w      worker, storage      connected
```

---

## Phase 1: Network Foundation and Authentication

### 1.1 Node Identity

Each LMB node has an identity based on an Ed25519 key pair:

- **NodeId** derived from libp2p `PeerId`
- Key pair generated on first startup, stored at `$XDG_DATA_HOME/lmb/identity.key`
- CA-issued X.509 certificate binds the `PeerId` to the node's identity

### 1.2 Transport Layer

Built on libp2p with the following protocol stack:

| Layer | Choice | Rationale |
|-------|--------|-----------|
| **Transport** | QUIC (libp2p-quic) | Built-in TLS 1.3, multiplexing, fast connection establishment |
| **Authentication** | TLS + custom certificate verifier | Verify peer certificate is signed by this network's CA and not on the CRL |
| **Discovery** | mDNS (LAN) + explicit bootstrap nodes | Closed network does not need DHT-based public discovery |
| **Broadcast** | gossipsub | CRL propagation, network announcements, role advertisements |
| **Point-to-point** | request-response | Work dispatch, CSR submission, CRL pull |

### 1.3 Permissioned Join Flow

```
1. Manager node initialization:
   $ lmb network init [--listen <addr>]
   - Generate CA root certificate (ca.crt) + CA private key (ca.key)
   - Generate local key pair (identity.key)
   - Sign own node certificate with CA (node.crt)
   - Write default config.toml
   - Output network fingerprint (SHA-256 of CA certificate) for verification

2. New node applies to join:
   $ lmb network join <manager-addr> [--fingerprint <sha256>]
   - Generate local key pair (identity.key)
   - Connect to manager via unauthenticated bootstrap channel
   - If --fingerprint provided, verify manager's CA fingerprint
   - Send CSR (Certificate Signing Request)
   - Manager queues the request as pending
   - Node enters waiting state, polls for approval

3. Manager approves:
   $ lmb network approve <node-id>
   - Sign certificate with CA private key
   - Pending node receives certificate + CA root cert on next poll
   (or)
   $ lmb network reject <node-id>
   - Reject application, clear pending record

4. New node comes online:
   - Store received node.crt + ca.crt
   - Connect to network via QUIC + mTLS
   - First action: pull latest CRL from manager (mandatory)
   - Before CRL sync completes: refuse connections to/from non-manager nodes
   - After CRL sync: participate in network normally
```

### 1.4 CRL Management

**Revocation:**

```
$ lmb network revoke <node-id>
- Manager adds certificate serial to CRL
- CRL version incremented, signed by manager
- Broadcast new CRL via gossipsub topic `lmb/crl`
```

**Sync strategy:**

| Scenario | Behavior |
|----------|----------|
| Node comes online | Mandatory CRL pull from manager |
| Manager unreachable | Pull from connected peers via request-response, accept highest version with valid signature |
| Normal operation | CRL updates broadcast via gossipsub `lmb/crl` in real-time |
| Periodic check | Each node checks CRL version periodically (default 1 hour), pulls update if behind |

**Validation rules:**

- Every CRL carries a version number + manager CA signature
- Nodes only accept CRL with version >= local version and valid signature
- Every new connection checks whether the peer's certificate is on the CRL
- Existing connections are dropped if a new CRL revokes the peer

### 1.5 CLI Commands

```
lmb network init [--listen <addr>]                  # Initialize as manager node
lmb network join <addr> [--fingerprint <sha256>]    # Apply to join network
lmb network start                                    # Start node, participate in network
lmb network stop                                     # Graceful stop
lmb network status                                   # Show network status, roles, connection count
lmb network peers [-v]                               # List connected peers (-v for details)
lmb network approve <node-id>                        # Approve pending node
lmb network reject <node-id>                         # Reject pending node
lmb network revoke <node-id>                         # Revoke node certificate
lmb network pending                                  # List pending approval requests
```

### 1.6 Module Architecture

```
src/network/
├── mod.rs                 # Public API, NetworkNode main struct
├── config.rs              # config.toml parsing, XDG path resolution
├── transport.rs           # libp2p transport setup (QUIC + mTLS)
├── behaviour.rs           # Custom NetworkBehaviour composition
│                          #   (mDNS + gossipsub + request-response)
├── auth/
│   ├── mod.rs             # Auth module public interface
│   ├── ca.rs              # CA operations (generate root, sign, revoke)
│   ├── cert.rs            # Certificate utilities (key pair gen, CSR gen, validation)
│   ├── crl.rs             # CRL management (versioning, signing, sync logic)
│   └── verifier.rs        # Custom TLS certificate verifier (CA validation + CRL check)
├── discovery.rs           # mDNS + bootstrap node discovery
├── signal.rs              # POSIX signal handling (tokio::signal)
└── protocol/
    ├── mod.rs             # Protocol definitions and versioning
    ├── bootstrap.rs       # Unauthenticated join protocol (CSR exchange, cert delivery)
    ├── crl_sync.rs        # CRL sync protocol (pull + gossip broadcast)
    └── codec.rs           # Message serialization (CBOR)
```

---

## Phase 2: Role System and Work Dispatch

### 2.1 Role Definitions

Three roles, all enabled by default:

| Role | Responsibility | Key Capabilities |
|------|---------------|------------------|
| **Manager** | Network administration | CA sign/revoke, CRL management, node approval, topology monitoring |
| **Storage** | Persistent data | Distributed Store access, data replication (Phase 3) |
| **Worker** | Execute work | Receive payloads (script/data/both), run Luau, return results |

**Configuration:**

```toml
[node]
name = "worker-01"                                   # Optional human-readable alias
roles = ["manager", "storage", "worker"]             # Default: all enabled
```

- Roles register corresponding libp2p protocol handlers at startup
- Nodes broadcast their roles via gossipsub, other nodes maintain a role index
- Roles can be dynamically adjusted via SIGHUP config reload (new roles take effect immediately, removed roles wait for in-progress work to complete)

### 2.2 Role Announcement and Discovery

```
Node comes online
  -> Broadcast via gossipsub topic `lmb/roles`:
    {
      peer_id: "12D3KooW...",
      name: "worker-01",              // optional alias
      roles: ["worker", "storage"],
      capacity: { max_concurrent: 4, available: 4 },
      version: "0.x.y"
    }

  -> Other nodes update local routing table
  -> Periodic heartbeat (default 30s) re-broadcasts with latest capacity
  -> 3 missed heartbeats -> mark as suspected offline
  -> 5 missed heartbeats -> remove from routing table
```

### 2.3 Work Dispatch Protocol

**Job structure:**

```
Job {
    id: UUID,
    mode: Sync | FireAndForget,
    payload: {
        script: Option<String>,      # Luau source code (optional)
        function: Option<String>,     # Name of deployed function (optional)
        state: Option<Value>,         # Input data/parameters (optional)
    },
    timeout: Option<Duration>,        # Execution timeout
    permissions: Permissions,         # Permission scope for this job
}
```

**Sync mode:**

```
Sender                              Worker
  |                                   |
  |---- JobRequest(job) ------------>|
  |                                   | Execute Luau
  |<--- JobResponse(result) ---------|
  |                                   |
```

- Uses libp2p request-response protocol
- Sender blocks until result or timeout
- Default timeout inherits node setting (30s), overridable per job

**Fire-and-forget mode:**

```
Sender                              Worker
  |                                   |
  |---- JobRequest(job) ------------>|
  |<--- JobAck(job_id) -------------|  (immediate acknowledgment)
  |                                   |  Execute Luau
  |                                   |
  |  (optional) query result:         |
  |---- JobQuery(job_id) ----------->|
  |<--- JobStatus(result|pending) ---|
```

- Sender returns after receiving Ack
- Worker stores result temporarily (configurable retention, default 1 hour)
- Sender can query result later via `JobQuery`
- Worker can optionally push result back to sender via request-response (if sender is still online)

### 2.4 Worker Scheduling

Workers manage a local job queue:

```
Receive JobRequest
  -> Check permissions (is the job within node's allowed scope?)
  -> Check script policy (does the job comply with accept_script setting?)
  -> Check capacity (any free slots?)
  |
  |-- Has capacity -> Accept, enqueue
  |                   -> Create Runner (reuse existing Runner architecture)
  |                   -> Execute, return result
  |
  |-- No capacity -> Reject: JobRejected(reason: AtCapacity)
  |                  -> Sender may retry with another Worker
  |
  |-- Policy violation -> Reject: JobRejected(reason: ScriptNotAllowed)
```

### 2.5 Worker Script Reception Control

Workers control what payloads they accept:

```toml
[worker]
accept_script = false          # Accept externally-sent scripts (default: false)
accept_state = true            # Accept external data (default: true)
allowed_functions = ["*"]      # Whitelist of callable local functions ("*" = all)
```

| `accept_script` | Payload contains script | Result |
|-----------------|------------------------|--------|
| `true` | Yes | Accept and execute |
| `false` | Yes | Reject: `JobRejected(reason: ScriptNotAllowed)` |
| `false` | No (state + function name only) | Accept, execute the named local function |

Default is `accept_script = false` for security. `allowed_functions` restricts which entry points are exposed, e.g. `["process", "transform"]`.

### 2.6 Worker Capacity and Permissions

```toml
[worker]
max_concurrent = 4          # Maximum concurrent jobs
queue_size = 16             # Waiting queue size (0 = no queue, reject when full)
default_timeout = "30s"     # Default job timeout

[worker.permissions]
# Permission ceiling for this worker — jobs cannot exceed these
allow_net = ["internal.api.example.com"]
deny_env = ["*"]
allow_read = ["/tmp"]
allow_write = []
```

A job's requested permissions are intersected with the worker's permission ceiling. If the job requests permissions beyond the ceiling, it is rejected.

### 2.7 Job Routing (Sender Logic)

```
Send Job
  -> Filter routing table for role=worker nodes
  -> Selection strategy (configurable):
  |
  |-- round-robin (default) — rotate across workers
  |-- least-loaded          — pick worker with highest available capacity
  |-- random                — random selection
  |
  -> Send JobRequest
  -> If rejected (AtCapacity) -> try next Worker
  -> All Workers rejected -> return error
```

```toml
[dispatch]
strategy = "round-robin"    # round-robin | least-loaded | random
max_retries = 3             # Retries on other Workers after rejection
```

### 2.8 Async Runtime and Thread Safety

The existing `Runner` holds a `Lua` VM instance that is `!Send` (cannot be transferred between threads). This affects the Worker executor design:

- **Job execution** must happen on a dedicated thread (or `spawn_local` on a `LocalSet`) since the Lua VM cannot cross thread boundaries
- **Worker executor** uses `tokio::task::spawn_blocking` or a dedicated thread pool where each thread owns its Lua VM
- **libp2p** and `openraft` both integrate with Tokio — the existing Tokio runtime is reused with no conflicts
- **Concurrency model**: The network event loop runs on Tokio async tasks, while Lua script execution is offloaded to blocking threads. Job requests and results are passed between the two via channels.

### 2.9 Security

- **Script transport**: Payloads encrypted in QUIC + mTLS channel
- **Execution sandbox**: Reuses existing LMB Luau sandbox (`vm.sandbox(true)`)
- **Permission isolation**: Each Job carries its own `Permissions`, Worker enforces against its ceiling

### 2.10 CLI Extensions

```
lmb network dispatch [--file <script.lua>] [--state <json>] [--sync] [--timeout <duration>]
lmb network dispatch --fire-and-forget [--file <script.lua>] [--state <json>]
lmb network job <job-id>              # Query async job result
lmb network workers [-v]              # List workers and their capacity
```

### 2.11 Luau API Extensions

New `@lmb/network` module:

```lua
local net = require("@lmb/network")

-- Sync dispatch
local result = net.dispatch({
    file = "process.lua",          -- send script (optional)
    state = { data = payload },    -- send data (optional)
    timeout = 10000,               -- timeout in ms (optional)
})

-- Async dispatch
local job_id = net.dispatch({
    file = "heavy_task.lua",
    state = { batch = items },
    async = true,
})

-- Query result
local status = net.job(job_id)
-- status.state: "pending" | "running" | "completed" | "failed"
-- status.result: execution result (when completed)
-- status.error: error message (when failed)
```

### 2.12 Module Architecture

```
src/network/
├── roles/
│   ├── mod.rs                # Role definitions, role registration logic
│   ├── announce.rs           # Role announcement and heartbeat (gossipsub)
│   └── router.rs             # Role routing table maintenance
├── work/
│   ├── mod.rs                # Work dispatch public API
│   ├── job.rs                # Job struct definition, serialization
│   ├── dispatch.rs           # Sender logic (routing strategy, retries)
│   ├── executor.rs           # Worker execution logic (queue, Runner integration)
│   └── result_store.rs       # Async job result temporary storage
├── protocol/
│   ├── roles.rs              # Role announcement protocol messages
│   └── work.rs               # Work dispatch protocol (Request/Response/Ack/Query)
```

### 2.13 Integration with Existing Architecture

| Existing Component | Integration |
|-------------------|-------------|
| **Runner** | Worker executor creates new Runner instances per Job via `Runner::builder()`. The existing `Pool`/`RunnerManager` cannot be directly reused because it requires a compile-time source (`AsChunk + Clone`), while Jobs carry dynamic script payloads. A new `JobExecutor` manages concurrency limits independently using a semaphore. |
| **StoreBackend** | Jobs can access local Store (if permissions allow) |
| **Permission** | Job Permissions reuse existing Permission struct. A new `Permissions::intersect()` method is needed to compute the effective permissions (intersection of Job-requested permissions and Worker ceiling). The existing `All { denied }` / `Some { allowed, denied }` variants require explicit intersection semantics: the result uses the more restrictive of the two for each permission category, with deny always taking precedence. |
| **serve** | Can coexist with network — HTTP serves local requests, network handles distributed work |

---

## Phase 3: Distributed Store (Raft Replication)

### 3.1 Goal

Maintain consistent key-value Store across multiple Storage nodes with strong consistency guarantees.

### 3.2 Consensus Mechanism

Raft consensus via `openraft` crate (most active Raft implementation in Rust ecosystem).

**Role mapping:**

| Raft Role | Mapping | Description |
|-----------|---------|-------------|
| **Leader** | Auto-elected | All writes must go through Leader |
| **Follower** | Other Storage nodes | Replicate Leader's log, can serve reads (depending on consistency level) |
| **Candidate** | When Leader is unreachable | Follower initiates election automatically |

### 3.3 Read/Write Flow

**Writes:**

```
Any node receives write request (store:set)
  |
  |-- This node is Leader
  |   -> Write to local WAL (Write-Ahead Log)
  |   -> Replicate to majority of Followers
  |   -> Majority confirmed -> commit -> respond success
  |
  |-- This node is not Leader
      -> Forward to Leader (via request-response)
      -> Leader processes and returns result
      -> Respond to caller
```

**Reads:**

```
store:get offers two consistency levels:

1. Strong consistency (default)
   -> Forward to Leader for read
   -> Guaranteed to read latest committed value

2. Eventual consistency (optional)
   -> Read directly from local Follower
   -> Low latency, but may read slightly stale value
```

### 3.4 Raft Network Layer Integration

Raft inter-node communication uses libp2p request-response directly (no separate TCP connections).

`openraft` requires implementing `RaftNetworkFactory` (which creates per-target `RaftNetwork` instances) backed by libp2p request-response. The following is simplified pseudocode — the actual `openraft` API uses `RaftTypeConfig` generics:

```rust
// Factory creates a network connection per target node
impl RaftNetworkFactory<TypeConfig> for LibP2pNetworkFactory {
    type Network = LibP2pRaftNetwork;

    async fn new_client(&mut self, target: NodeId, node: &Node)
        -> Self::Network {
        // Return a libp2p-backed network client for the target node
        LibP2pRaftNetwork { swarm: self.swarm.clone(), target }
    }
}

// Per-target network handles Raft RPCs via libp2p request-response
impl RaftNetwork<TypeConfig> for LibP2pRaftNetwork {
    async fn append_entries(&mut self, rpc: AppendEntriesRequest<TypeConfig>)
        -> Result<AppendEntriesResponse<TypeConfig>>;
    async fn vote(&mut self, rpc: VoteRequest<TypeConfig>)
        -> Result<VoteResponse<TypeConfig>>;
    async fn install_snapshot(&mut self, vote: Vote<TypeConfig>, snapshot: Snapshot)
        -> Result<InstallSnapshotResponse<TypeConfig>>;
}
```

### 3.5 State Machine

`openraft`'s `RaftStateMachine` backed by existing `StoreBackend` trait.

**Sync/async impedance mismatch:** The existing `StoreBackend` trait methods are synchronous (`fn get`, `fn put`, `fn del`), while `openraft`'s state machine trait uses async methods. The implementation bridges this via `spawn_blocking` to run synchronous store operations on the blocking thread pool, preventing them from blocking the Tokio async runtime.

Simplified pseudocode:

```rust
impl RaftStateMachine<TypeConfig> for StoreStateMachine {
    async fn apply(entries: Vec<Entry>) -> Vec<Response> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            for entry in entries {
                match entry {
                    Put(key, value) => store.put(key, value),
                    Del(key)        => store.del(key),
                }
            }
        }).await
    }

    async fn snapshot() -> Snapshot {
        // Export all key-value pairs as snapshot (via spawn_blocking)
    }

    async fn install_snapshot(snapshot: Snapshot) {
        // Clear local store, import snapshot (via spawn_blocking)
    }
}
```

Each Storage node still uses SQLite or PostgreSQL as local storage engine. Raft ensures write ordering consistency across nodes.

### 3.6 Cluster Management

**Initialization:**

- First Storage node automatically becomes single-node Raft cluster Leader
- Subsequent Storage nodes discover Leader via role announcements
- Leader adds new nodes via Raft joint consensus membership change

**Minimum node counts:**

| Storage Nodes | Tolerable Failures | Notes |
|--------------|-------------------|-------|
| 1 | 0 | Single node, no redundancy (default experience mode) |
| 2 | 0 | Cannot form majority, not recommended |
| 3 | 1 | Minimum HA configuration |
| 5 | 2 | Recommended for production |

**Key design decision:** In single-node deployment (one Storage node or network not enabled), Store operates directly on local backend without Raft, preserving existing performance. Raft layer activates only when multiple Storage nodes are detected.

### 3.7 Distributed Transactions

Transactions are implemented as batch-applied Raft log entries.

**Reconciling with existing tx API:** The current `store:tx(function(tx) ... end)` Luau API executes operations eagerly within the callback via `begin_tx()` / `commit_tx()` / `rollback_tx()`. In distributed mode, this must change to deferred execution:

1. When Raft is active, the `tx` object inside the callback records operations into a write buffer instead of executing them immediately
2. Reads within a transaction (`tx:get`) read from the buffer first (read-your-writes), falling back to the underlying store
3. When the callback completes without error, the buffered operations are packaged as a single `BatchWrite` Raft log entry
4. If the callback errors, the buffer is discarded (no Raft entry submitted)

This means the `store.rs` Luau binding needs a `TransactionBuffer` layer that intercepts `StoreBackend` calls when in distributed mode:

```lua
store:tx(function(tx)
    tx:set("a", 1)        -- buffered, not yet written
    tx:set("b", 2)        -- buffered
    local a = tx:get("a") -- reads from buffer -> returns 1
end)
-- callback completed -> submit BatchWrite([Put("a",1), Put("b",2)]) to Raft
-- Raft replicates to majority -> atomic apply on all nodes
```

In single-node mode (no Raft), the existing eager `begin_tx()` / `commit_tx()` / `rollback_tx()` behavior is preserved unchanged.

### 3.8 Snapshots and Recovery

- **Snapshot trigger**: Auto-snapshot when WAL log exceeds threshold (default 10,000 entries)
- **Snapshot format**: Full key-value dump (MessagePack serialization)
- **New node joining**: Leader sends latest snapshot first, then replicates from post-snapshot log
- **Recovery**: Node restarts from local latest snapshot + WAL replay

### 3.9 Luau API Extensions

Existing `store` API semantics unchanged, with new consistency option:

```lua
local store = ctx.store

-- Writes (always go through Raft Leader)
store:set("key", value)
store:del("key")
store:tx(function(tx)
    tx:set("a", 1)
    tx:set("b", 2)
end)

-- Reads (strong consistency by default)
local v = store:get("key")

-- Eventual consistency read (low latency)
-- Note: store:get already accepts an optional second argument (options table
-- with `default` key). The `consistency` key is added to the same options table.
local v = store:get("key", { consistency = "eventual" })
local v = store:get("key", { default = "fallback", consistency = "eventual" })

-- Query cluster status
local net = require("@lmb/network")
local info = net.store_status()
-- info.role: "leader" | "follower"
-- info.leader_id: current Leader's PeerId
-- info.members: list of Storage nodes
-- info.commit_index: latest committed log index
```

### 3.10 Configuration

```toml
[storage]
heartbeat_interval = "150ms"       # Leader heartbeat interval
election_timeout_min = "300ms"     # Election timeout lower bound
election_timeout_max = "500ms"     # Election timeout upper bound
snapshot_threshold = 10000         # Log entries before auto-snapshot
default_consistency = "strong"     # strong | eventual
```

### 3.11 Module Architecture

```
src/network/
├── consensus/
│   ├── mod.rs                # Public API, RaftNode initialization
│   ├── network.rs            # RaftNetwork trait impl (libp2p transport)
│   ├── state_machine.rs      # RaftStateMachine impl (backed by StoreBackend)
│   ├── log_store.rs          # Raft log storage (SQLite or dedicated WAL file)
│   └── membership.rs         # Cluster membership management (join/remove/change)
├── protocol/
│   └── raft.rs               # Raft RPC message definitions and serialization
```

### 3.12 Integration with Existing Architecture

| Existing Component | Integration |
|-------------------|-------------|
| **StoreBackend trait** | Serves as Raft state machine's underlying storage engine, interface unchanged |
| **SQLiteBackend** | Direct use in single-node mode (no Raft), local storage engine under Raft in multi-node |
| **PostgresBackend** | Same as SQLite — can serve as Raft local storage engine |
| **store binding** | Luau API semantics unchanged, automatically routes through Raft when applicable |
| **Pool (serve)** | serve mode Store access also goes through Raft (if Storage role is enabled) |

---

## Phase 4: Operations Tooling and Monitoring

### 4.1 Goal

Provide observability, diagnostic tools, and management capabilities for production operation of the network.

### 4.2 Network Status

**`lmb network status` output:**

```
Node:     12D3KooWA1b2...
Name:     worker-01
Roles:    manager, storage, worker
Uptime:   3d 14h 22m
Listen:   /ip4/0.0.0.0/udp/4001/quic-v1

Network:
  Peers:       12 connected, 15 known
  CRL:         v7 (updated 4m ago)

Storage (Raft):
  Role:        follower
  Leader:      12D3KooWB3c4...
  Members:     3/3 healthy
  Commit:      #48291

Worker:
  Running:     2/4
  Queued:      1
  Completed:   1,847 (today)
  Failed:      3 (today)
```

### 4.3 Structured Logging

Extends existing `tracing` framework with network-specific spans and events:

```toml
[logging]
level = "info"
network = "info"          # Connections, disconnections, peer discovery
auth = "warn"             # Certificate validation, CRL updates
consensus = "info"        # Raft elections, log replication
work = "info"             # Job dispatch, execution
```

**Critical events (always logged):**

| Event | Level | Description |
|-------|-------|-------------|
| Node join/leave | info | Includes PeerId and roles |
| Certificate issued/revoked | warn | Security-related operations |
| CRL update | info | Includes version number |
| Raft Leader change | warn | Cluster state change |
| Job completed | info | Includes job_id, duration, memory usage |
| Job failed | error | Includes job_id, error message |
| Connection rejected by CRL | warn | Includes peer's PeerId |

### 4.4 Health Check

Optional local health check endpoint:

```toml
[health]
enabled = true
bind = "127.0.0.1:9090"
```

```
GET /health -> 200 OK | 503 Service Unavailable
{
  "status": "healthy",
  "roles": ["manager", "storage", "worker"],
  "peers": 12,
  "raft_state": "follower",
  "crl_version": 7,
  "worker_available": 2
}
```

Binds to localhost only, not exposed to network. Suitable for external monitoring (Prometheus, load balancers).

### 4.5 Metrics Export (Optional, Feature-gated)

```toml
[metrics]
enabled = false
bind = "127.0.0.1:9091"
```

Prometheus-format metrics:

| Metric | Type | Description |
|--------|------|-------------|
| `lmb_peers_connected` | gauge | Connected peer count |
| `lmb_jobs_total` | counter | Total jobs (labeled by status) |
| `lmb_job_duration_seconds` | histogram | Job execution duration |
| `lmb_raft_commit_index` | gauge | Raft commit progress |
| `lmb_raft_leader_changes_total` | counter | Leader change count |
| `lmb_crl_version` | gauge | Current CRL version |

### 4.6 CLI Extensions

```
# Diagnostics
lmb network ping <node-id>                 # Test connectivity and latency to a specific node
lmb network logs [-f] [--level <level>]     # View network events in real-time (-f = follow)

# Job management
lmb network jobs [--status <status>]        # List job records on this node
lmb network jobs --purge [--before <time>]  # Clean up expired job records

# Cluster management
lmb network store-status                    # Raft cluster status summary
```

### 4.7 Module Architecture

```
src/network/
├── monitor/
│   ├── mod.rs               # Status aggregation, public query API
│   ├── health.rs            # Health check HTTP endpoint
│   └── metrics.rs           # Prometheus metrics export (feature-gated)
├── cli/
│   ├── mod.rs               # network subcommand definitions
│   ├── init.rs              # network init handler
│   ├── join.rs              # network join handler
│   ├── manage.rs            # approve/reject/revoke handlers
│   ├── status.rs            # status/peers/workers display
│   └── jobs.rs              # jobs query and cleanup
```

---

## Complete Configuration Reference

```toml
[node]
name = "my-node"                                     # Optional human-readable alias
roles = ["manager", "storage", "worker"]             # Default: all enabled

[network]
listen = "/ip4/0.0.0.0/udp/4001/quic-v1"
bootstrap = []

[network.mdns]
enabled = true

[network.crl]
sync_interval = "1h"       # Periodic CRL sync interval
grace_period = "30s"       # CRL sync grace period after coming online

[worker]
max_concurrent = 4          # Maximum concurrent jobs
queue_size = 16             # Waiting queue size (0 = no queue)
default_timeout = "30s"     # Default job timeout
accept_script = false       # Accept externally-sent scripts (default: false)
accept_state = true         # Accept external data (default: true)
allowed_functions = ["*"]   # Whitelist of callable local functions

[worker.permissions]
allow_net = []
deny_env = ["*"]
allow_read = ["/tmp"]
allow_write = []

[dispatch]
strategy = "round-robin"    # round-robin | least-loaded | random
max_retries = 3             # Retries on other Workers after rejection

[storage]
heartbeat_interval = "150ms"
election_timeout_min = "300ms"
election_timeout_max = "500ms"
snapshot_threshold = 10000
default_consistency = "strong"     # strong | eventual

[health]
enabled = true
bind = "127.0.0.1:9090"

[metrics]
enabled = false
bind = "127.0.0.1:9091"

[logging]
level = "info"

[shutdown]
timeout = "30s"             # Graceful shutdown max wait time
```

---

## Complete Module Architecture

```
src/network/
├── mod.rs                     # Public API, NetworkNode main struct
├── config.rs                  # config.toml parsing, XDG path resolution
├── transport.rs               # libp2p transport setup (QUIC + mTLS)
├── behaviour.rs               # Custom NetworkBehaviour composition
├── discovery.rs               # mDNS + bootstrap node discovery
├── signal.rs                  # POSIX signal handling (tokio::signal)
├── auth/
│   ├── mod.rs                 # Auth module public interface
│   ├── ca.rs                  # CA operations (generate root, sign, revoke)
│   ├── cert.rs                # Certificate utilities (key pair gen, CSR gen, validation)
│   ├── crl.rs                 # CRL management (versioning, signing, sync logic)
│   └── verifier.rs            # Custom TLS certificate verifier (CA + CRL check)
├── roles/
│   ├── mod.rs                 # Role definitions, role registration logic
│   ├── announce.rs            # Role announcement and heartbeat (gossipsub)
│   └── router.rs              # Role routing table maintenance
├── work/
│   ├── mod.rs                 # Work dispatch public API
│   ├── job.rs                 # Job struct definition, serialization
│   ├── dispatch.rs            # Sender logic (routing strategy, retries)
│   ├── executor.rs            # Worker execution logic (queue, Runner integration)
│   └── result_store.rs        # Async job result temporary storage
├── consensus/
│   ├── mod.rs                 # Public API, RaftNode initialization
│   ├── network.rs             # RaftNetwork trait impl (libp2p transport)
│   ├── state_machine.rs       # RaftStateMachine impl (backed by StoreBackend)
│   ├── log_store.rs           # Raft log storage
│   └── membership.rs          # Cluster membership management
├── monitor/
│   ├── mod.rs                 # Status aggregation, public query API
│   ├── health.rs              # Health check HTTP endpoint
│   └── metrics.rs             # Prometheus metrics export (feature-gated)
├── protocol/
│   ├── mod.rs                 # Protocol definitions and versioning
│   ├── bootstrap.rs           # Unauthenticated join protocol (CSR exchange)
│   ├── crl_sync.rs            # CRL sync protocol (pull + gossip)
│   ├── roles.rs               # Role announcement protocol messages
│   ├── work.rs                # Work dispatch protocol messages
│   ├── raft.rs                # Raft RPC message definitions
│   └── codec.rs               # Message serialization (CBOR)
└── cli/
    ├── mod.rs                 # network subcommand definitions
    ├── init.rs                # network init handler
    ├── join.rs                # network join handler
    ├── manage.rs              # approve/reject/revoke handlers
    ├── status.rs              # status/peers/workers display
    └── jobs.rs                # jobs query and cleanup
```
