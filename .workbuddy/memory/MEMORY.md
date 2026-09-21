# tcp-transfer 项目长期约定

## 项目性质
轻量 TCP 端口转发工具（Rust / tokio / clap / tracing），单二进制 `tcp-transfer`。
**主要用途：测试环境下的端口转发**（把打进本机端口的连接透明转发到另一台机器的端口）。

## CLI 约定（2026-09-21 最终定稿：极简两参数）
```
tcp-transfer -l 0.0.0.0:444 -t 192.168.2.203:333
```
- `-l/--listen`（必填）**完整 `host:port`**，类型 `SocketAddr` → clap 启动前就校验格式
- `-t/--target`（必填）**完整 `host:port`**，类型 `String` → 必须支持域名，不能用 SocketAddr
- `-T/--timeout`（默认 0=不超时）、`-v/--log-level`（默认 info）、`--json-log`
- IPv6 目标需方括号：`-t [::1]:80`

**关键设计（勿"统一"）**：`listen` 用 `SocketAddr` 是为了启动前拒绝漏端口的输入；
`target` 用 `String` 是因为域名（example.com:80）无法解析为 `SocketAddr`。

（历史演进，均已移除，不要恢复：① 最早的 `forward -l 0.0.0.0:8080 -t host:port` 子命令；
② 中间的 `-p 100 --target X --target-port 2000` 三段式。用户明确要求最简参数。）

**副产品收益**：三段式时代的缺陷「`--target X:2000` 拼出畸形地址 `X:2000:100`」
在改成完整 host:port 后**自然消失**（不再有端口拼接逻辑）。

## 构建约定
- **Linux 构建统一走 `Dockerfile-linux`**，基础镜像固定为内网私服 `zhcoder-docker-registry.com:8000/builder/rust:1.96-slim-musl`。
- 本机构建命令：
  ```
  docker build --pull=false -f Dockerfile-linux -t tcp-transfer:linux .
  ```
  `--pull=false` 是必需的（见下方私服限制）。
- 产物导出到 `dist/tcp-transfer`（已在 .gitignore 中忽略）。

## 内网私服限制（zhcoder-docker-registry.com:8000）
- **仅支持明文 HTTP，不支持 HTTPS/TLS**。
- Harbor 仓库，token 端点 `/service/token`。
- Docker 29 执行 `docker login` 该私服会失败（`503 Service Unavailable`）——Harbor 与本机 Docker 29 的 API ping 不兼容。
- **结论：依赖本地已有镜像 + `--pull=false` 构建，不依赖 `docker login`**，这是当前唯一可行路径。

## 关键构建知识
- 基础镜像的 per-target `rustflags` 数组会**替换**（非合并）`RUSTFLAGS` 环境变量；要加编译参数必须 `sed` 追加到 `~/.cargo/config.toml` 的数组里。
- 静态链接判定**不能用 musl 的 `ldd`**（会对静态二进制误报 loader 路径）。正确方法：`readelf -l` 查 `PT_INTERP`，或放进 `FROM scratch` 跑一次。
- Windows Git Bash 下调 docker 命令需 `MSYS_NO_PATHCONV=1` 防路径改写。

## 文档分工（2026-09-21 确立）
- **`README.md`** = 面向**用户**的产品用法（参数表、构建方式、示例）
- **`AGENTS.md`** = 面向 **AI 编码代理**的操作性约定（架构职责、验证纪律、已知陷阱、改动红线）
- 两者不要互相复制内容；改了一处记得检查另一处是否需同步。

## 已知缺陷（尚未修）
- 仓库**无任何自动化测试**、无 CI。改转发逻辑必须手工端到端验证（方法见 AGENTS.md 第 6 节）。
- 仅支持 TCP，无 UDP；一次进程只跑一条转发规则；无访问控制（勿暴露公网）。

## 验证方法学（重要经验）
- 大流量完整性测试**不要用 `nc -l` 循环装置**，有时序缺陷会给出 0 字节假失败；用 python socket 比对 MD5。
- 怀疑转发器丢数据时，**先验证直连后端是否也异常**——若直连同样异常，问题在测试装置而非代码。
- **测量退出码不要接管道**（`cmd | tail` 会吞掉退出码，误得 0）；用 `cmd >/dev/null 2>&1; echo $?`。
- `cargo test` 可以跑但用例数为 0，**别据此以为有测试覆盖**。

## 运行行为事实（供文档/排查参考）
- 退出码：0 = 正常；1 = 启动失败（bind 失败等）。运行期单连接错误**不退出**，只 warn。
- `--timeout` 是**单次读的空闲上限**，不是连接总时长。
- 字节口径以**监听侧**为参照：`bytes_in` = 客户端→目标，`bytes_out` = 目标→客户端。
- 统计日志每 10 秒一条。
