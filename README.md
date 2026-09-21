# tcp-transfer

轻量 TCP 端口转发工具，主要用于**测试场景**：把打进本机某个端口的连接，透明转发到另一台机器的另一个端口。

```
客户端 ──→ 本机 :444 ──[tcp-transfer]──→ 目标主机 :333
```

典型用途：临时把线上/预发的服务映射到本地端口做联调、绕开防火墙做连通性测试、给固定端口的旧客户端指向新后端。

---

## 快速开始

场景：客户端访问 `192.168.2.10:444`，但真实服务在 `192.168.2.203:333`。在 `192.168.2.10` 上执行：

```bash
tcp-transfer -l 0.0.0.0:444 -t 192.168.2.203:333
```

客户端连 `192.168.2.10:444` 即可，流量会被转发到 `192.168.2.203:333`。

启动后会打印实际监听/转发信息，**建议每次都看一眼确认转发目标对不对**：

```
INFO tcp_transfer::proxy: listening listen=0.0.0.0:444 target=192.168.2.203:333 idle_timeout=0
```

---

## 参数

| 参数 | 简写 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `--listen` | `-l` | ✅ | — | 本机监听地址，**必须带端口**，如 `0.0.0.0:444` |
| `--target` | `-t` | ✅ | — | 转发目标，`host:port` 形式 |
| `--timeout` | `-T` | | `0` | 空闲读超时（秒），0 = 不超时 |
| `--log-level` | `-v` | | `info` | error / warn / info / debug / trace |
| `--json-log` | | | false | 输出结构化 JSON 日志 |

监听地址和目标地址**都写成完整的 `host:port`**，不再拆成两个参数。

### 常用变体

```bash
# 只允许本机连接（更安全，适合单机调试）
tcp-transfer -l 127.0.0.1:444 -t 192.168.2.203:333

# 目标用域名（会自动解析）
tcp-transfer -l 0.0.0.0:8080 -t api.example.com:80

# 带 30 秒空闲超时 + debug 日志（排查连接问题时用）
tcp-transfer -l 0.0.0.0:444 -t 192.168.2.203:333 -T 30 -v debug

# 结构化 JSON 日志（便于被日志系统采集）
tcp-transfer -l 0.0.0.0:444 -t 192.168.2.203:333 --json-log
```

**IPv6 写法**（目标为 IPv6 时需要方括号）：

```bash
tcp-transfer -l 0.0.0.0:444 -t [::1]:80
```

> ⚠️ `-l` 会严格校验格式，写成 `0.0.0.0`（漏端口）会直接报错：
> `invalid value '0.0.0.0' for '--listen <LISTEN>': invalid socket address syntax`

---

## 运行行为

**日志**
每 10 秒输出一次统计（累计连接数、当前活跃连接、双向字节数）：

```
INFO tcp_transfer::proxy: stats total=1 active=0 bytes_in="0.00 B" bytes_out="1.00 MiB"
```

> 字节数口径**以监听侧为参照系**：`bytes_in` = 客户端 → 目标，`bytes_out` = 目标 → 客户端。

**退出码**

| 码 | 含义 |
|---|---|
| `0` | 正常退出 |
| `1` | 启动失败（如端口被占用，日志会打 `failed to bind 0.0.0.0:444`） |

**错误处理**
程序**常驻**运行，不会因单个连接出错而退出：

- 端口被占用 → 立即报错退出（退出码 1）
- 某个连接的目标主机连不上 → 该连接结束并记 `warn`，**服务继续监听**
- accept 出错 → 记 `warn` 后继续

**超时**
`--timeout` 是**单次读的空闲上限**，不是整个连接的总时长。设为 `30` 表示某方向 30 秒没数据就断开该方向。长连接场景（如 SSH、WebSocket）建议保持默认 `0`。

---

## 构建

### Linux（静态二进制，推荐用于服务器）

```bash
docker build --pull=false -f Dockerfile-linux -t tcp-transfer:linux .
```

产出 `x86_64-unknown-linux-musl` **完全静态** 二进制，不依赖任何运行库，可直接丢到任意 Linux 发行版（glibc / musl 均可）上运行——**无需安装任何依赖，也无需目标机器有 Rust 环境**。

导出二进制到 `dist/`：

```bash
docker create --name tcp-transfer-tmp tcp-transfer:linux
docker cp tcp-transfer-tmp:/usr/local/bin/tcp-transfer dist/tcp-transfer
docker rm tcp-transfer-tmp
```

然后拷到目标服务器：

```bash
scp dist/tcp-transfer user@192.168.2.10:/usr/local/bin/
ssh user@192.168.2.10 'chmod +x /usr/local/bin/tcp-transfer'
```

> **Windows Git Bash 用户注意**：执行 `docker cp` 请加 `MSYS_NO_PATHCONV=1` 前缀，
> 否则路径会被 MSYS 错误改写：
> ```bash
> MSYS_NO_PATHCONV=1 docker cp tcp-transfer-tmp:/usr/local/bin/tcp-transfer dist/tcp-transfer
> ```

### 直接用镜像运行

```bash
docker run --rm -p 444:444 tcp-transfer:linux \
  -l 0.0.0.0:444 -t 192.168.2.203:333
```

> 用 `--network host` 可省去端口映射，且能直接访问宿主机的局域网 IP：
> ```bash
> docker run --rm --network host tcp-transfer:linux \
>   -l 0.0.0.0:444 -t 192.168.2.203:333
> ```

### 本地原生构建

```bash
cargo build --release
# 产物：target/release/tcp-transfer       (Linux/macOS)
#       target/release/tcp-transfer.exe   (Windows)
```

---

## 后台运行

### Linux（systemd）

`/etc/systemd/system/tcp-transfer.service`：

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
journalctl -u tcp-transfer -f        # 看日志
```

### Linux（nohup，临时用）

```bash
nohup tcp-transfer -l 0.0.0.0:444 -t 192.168.2.203:333 \
  > /var/log/tcp-transfer.log 2>&1 &
```

---

## 故障排查

| 现象 | 原因 | 处理 |
|---|---|---|
| `invalid socket address syntax` | `-l` 漏写端口 | 写成 `0.0.0.0:444` 而非 `0.0.0.0` |
| `failed to bind 0.0.0.0:444` | 端口已被占用 | `netstat -tlnp \| grep 444` 找到占用进程，或换端口 |
| 客户端连不上监听端口 | 绑了 `127.0.0.1` 或防火墙拦截 | 确认 `-l` 第一个字段为 `0.0.0.0`；检查防火墙 |
| 连接被立刻关闭 | 目标主机/端口不可达 | 先在转发机上 `telnet <target> <port>` 验证直连是否通 |
| 长连接几十秒后断开 | `--timeout` 设得太小 | 改回 `0` 或调大 |
| 连上了但数据不对 | 目标地址填错 | 看启动日志的 `target=` 字段核对 |

**通用排查思路**：先在转发机上**直连目标**验证通路，再走转发器对比。若直连就不通，问题不在转发器。

---

## 开发

```bash
cargo build --release               # 构建
cargo clippy -- -D warnings         # 静态检查
```

> **当前仓库尚无自动化测试**（`cargo test` 可执行但用例数为 0），
> 改动转发逻辑后请按 `AGENTS.md` 第 6 节做手工端到端验证。

面向 AI 编码代理的操作性约定（架构职责、验证模板、已知陷阱、改动红线）见 **`AGENTS.md`**。

---

## 限制

- **仅支持 TCP**，不含 UDP
- 一次进程只跑一条转发规则（不支持多规则批量转发）
- 无访问控制 / 认证（**请勿直接暴露到公网**）

---

## License

MIT
