# tcp-transfer

A lightweight TCP port-forwarding tool, mainly for **testing scenarios**: transparently forward connections hitting a local port to another machine's port.

```
client ──→ local :444 ──[tcp-transfer]──→ target host :333
```

Typical use cases: temporarily expose a staging/production service on a local port for debugging, route around firewall blocks for connectivity tests, or point a fixed-port legacy client at a new backend.

> 📘 **Looking for the Chinese version?** See [README.md](README.md).

---

## Quick Start

Scenario: the client wants to reach `192.168.2.203:333`, but you only have access to `192.168.2.10`. Run on `192.168.2.10`:

```bash
tcp-transfer -l 0.0.0.0:444 -t 192.168.2.203:333
```

The client connects to `192.168.2.10:444` and the traffic is forwarded to `192.168.2.203:333`.

On startup, the actual listen/target endpoints are printed. **Always double-check them to confirm you're forwarding to the right destination:**

```
INFO tcp_transfer::proxy: listening listen=0.0.0.0:444 target=192.168.2.203:333 idle_timeout=0
```

---

## Options

| Flag | Short | Required | Default | Description |
|---|---|---|---|---|
| `--listen` | `-l` | ✅ | — | Local listen address. **Must include the port**, e.g. `0.0.0.0:444` |
| `--target` | `-t` | ✅ | — | Forwarding target, `host:port` form |
| `--timeout` | `-T` | | `0` | Idle read timeout (seconds). `0` = no timeout |
| `--log-level` | `-v` | | `info` | error / warn / info / debug / trace |
| `--json-log` | | | false | Emit structured JSON logs |

Both the listen address and the target address are written as a complete `host:port` — they are no longer split into two flags.

### Common variants

```bash
# Local-only access (safer, good for single-host debugging)
tcp-transfer -l 127.0.0.1:444 -t 192.168.2.203:333

# Target by hostname (resolved automatically)
tcp-transfer -l 0.0.0.0:8080 -t api.example.com:80

# 30-second idle timeout + debug logging (for connection troubleshooting)
tcp-transfer -l 0.0.0.0:444 -t 192.168.2.203:333 -T 30 -v debug

# Structured JSON logs (for log collectors)
tcp-transfer -l 0.0.0.0:444 -t 192.168.2.203:333 --json-log
```

**IPv6 form** (brackets are required for IPv6 targets):

```bash
tcp-transfer -l 0.0.0.0:444 -t [::1]:80
```

> ⚠️ `-l` validates strictly. Omitting the port (`0.0.0.0`) is rejected immediately:
> `invalid value '0.0.0.0' for '--listen <LISTEN>': invalid socket address syntax`

---

## Runtime Behavior

**Logs**
Every 10 seconds, statistics are printed (total connections, currently active connections, bytes in both directions):

```
INFO tcp_transfer::proxy: stats total=1 active=0 bytes_in="0.00 B" bytes_out="1.00 MiB"
```

> Byte counts are measured **from the listener's perspective**: `bytes_in` = client → target, `bytes_out` = target → client.

**Exit codes**

| Code | Meaning |
|---|---|
| `0` | Normal exit |
| `1` | Startup failure (e.g. port already in use; the log will show `failed to bind 0.0.0.0:444`) |

**Error handling**
The process runs as a daemon and **will not exit because of a single connection error**:

- Port already in use → fail fast on startup (exit code 1)
- A connection's target host is unreachable → that connection ends with a `warn`, **the listener keeps running**
- `accept` failure → log `warn` and keep running

**Timeout**
`--timeout` is the **per-read idle cap**, not a total connection lifetime. A value of `30` means: if either direction sees no data for 30 seconds, close that direction. For long-lived connections (SSH, WebSocket, etc.) keep the default `0`.

---

## Building

### Linux (static binary, recommended for servers)

```bash
docker build --pull=false -f Dockerfile-linux -t tcp-transfer:linux .
```

The output is an `x86_64-unknown-linux-musl` **fully static** binary with no runtime dependencies. It runs on any Linux distribution (glibc or musl alike) — **no dependencies need to be installed, and the target machine does not need a Rust toolchain**.

Export the binary to `dist/`:

```bash
docker create --name tcp-transfer-tmp tcp-transfer:linux
docker cp tcp-transfer-tmp:/usr/local/bin/tcp-transfer dist/tcp-transfer
docker rm tcp-transfer-tmp
```

Then copy it to the target server:

```bash
scp dist/tcp-transfer user@192.168.2.10:/usr/local/bin/
ssh user@192.168.2.10 'chmod +x /usr/local/bin/tcp-transfer'
```

> **Windows Git Bash note**: when running `docker cp`, prefix with `MSYS_NO_PATHCONV=1`,
> otherwise MSYS will mangle the path:
> ```bash
> MSYS_NO_PATHCONV=1 docker cp tcp-transfer-tmp:/usr/local/bin/tcp-transfer dist/tcp-transfer
> ```

### Run directly from the image

```bash
docker run --rm -p 444:444 tcp-transfer:linux \
  -l 0.0.0.0:444 -t 192.168.2.203:333
```

> Use `--network host` to skip port mapping and reach the host's LAN IP directly:
> ```bash
> docker run --rm --network host tcp-transfer:linux \
>   -l 0.0.0.0:444 -t 192.168.2.203:333
> ```

### Native build on the host

```bash
cargo build --release
# Output: target/release/tcp-transfer       (Linux/macOS)
#         target/release/tcp-transfer.exe   (Windows)
```

---

## Running in the Background

### Linux (systemd)

`/etc/systemd/system/tcp-transfer.service`:

```ini
[Unit]
Description=TCP port forwarder (444 -> 192.168.2.203:333)
After=network-online.target

[Service]
ExecStart=/usr/local/bin/tcp-transfer -l 0.0.0.0:444 -t 192.168.2.203:333
Restart=always
RestartSec=3
DynamicUser=yes

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now tcp-transfer
journalctl -u tcp-transfer -f        # tail logs
```

### Linux (nohup, for ad-hoc use)

```bash
nohup tcp-transfer -l 0.0.0.0:444 -t 192.168.2.203:333 \
  > /var/log/tcp-transfer.log 2>&1 &
```

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `invalid socket address syntax` | `-l` is missing the port | Write `0.0.0.0:444`, not just `0.0.0.0` |
| `failed to bind 0.0.0.0:444` | Port already in use | `netstat -tlnp \| grep 444` to find the holder, or pick another port |
| Client can't reach the listen port | Bound to `127.0.0.1` or blocked by firewall | Confirm the first field of `-l` is `0.0.0.0`; check firewall rules |
| Connection closed immediately | Target host/port unreachable | `telnet <target> <port>` from the forwarder first to verify direct connectivity |
| Long connection drops after tens of seconds | `--timeout` set too low | Reset to `0` or raise the value |
| Connected but data is wrong | Wrong target address | Verify the `target=` field in the startup log |

**General debugging rule**: from the forwarder, **connect to the target directly** first to verify reachability, then compare with the forwarded path. If direct access is broken, the issue is not in the forwarder.

---

## Development

```bash
cargo build --release               # build
cargo clippy -- -D warnings         # static checks
```

> **The repo currently has no automated tests** (`cargo test` runs but with 0 cases).
> After changing forwarding logic, follow the manual end-to-end verification procedure in
> section 6 of **`AGENTS.md`**.

Operational conventions for AI coding agents (architecture boundaries, verification templates, known pitfalls, change red lines) are in **`AGENTS.md`**.

---

## Limitations

- **TCP only**, no UDP
- One process = one forwarding rule (no bulk rule sets)
- No access control / authentication (**do not expose directly to the public internet**)

---

## License

MIT
