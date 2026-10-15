# RustDesk GX 联合发布：服务器部署

这份指南部署同一个 RustDesk GX Release 中的配套服务，不会自动执行命令或改动现有服务器。首次部署示例使用 **Ubuntu 24.04 x86_64**、systemd 管理 hbbs/hbbr、Docker Compose 管理可选账号 API。已有部署请先阅读末尾的升级步骤，不要照搬首次初始化命令。

## 下载哪些文件

在 [本 fork 的 Releases](https://github.com/GX-Swjtu/rustdesk/releases) 选择同一版的文件；不要混用官方服务端、不同联合版本或还未完成的草稿。

| 附件 | 用途 |
| --- | --- |
| `rustdesk-server-<服务端版本>-x86_64-unknown-linux-gnu.tar.gz` | hbbs、hbbr、rustdesk-utils 和源码身份 |
| `rustdesk-account-server-<账号版本>-x86_64-unknown-linux-gnu.tar.gz` | 可选账号服务原生程序及 `deploy/` 配置 |
| `rustdesk-account-server-<账号版本>-docker-linux-x86_64.tar.gz` | 可选账号服务的本地容器镜像 |
| `rustdesk-<客户端版本>-server-guide.zip` | 本指南、客户端配置指南及 hbbs/hbbr systemd 模板 |
| `suite-components.json`、`SHA256SUMS` | 本次组件版本、完整提交与下载校验 |
| 两个 `rustdesk-hbb-common-*.tar.gz` | 客户端和服务端各自的共享库源码，包含 Native P2P 模块及第三方声明；部署时无需安装 |

`hbb_common` 已编入客户端/hbbs 等程序，没有单独启动的进程。客户端与服务端使用不同的上游兼容基线是有意设计；不能把一个源码归档覆盖到另一个项目中。

在独立下载目录中校验已下载文件：

```sh
sha256sum --ignore-missing --check SHA256SUMS
CLIENT_VERSION=1.5.1-gx.4 # 示例：替换为所下载的真实客户端版本
unzip "rustdesk-${CLIENT_VERSION}-server-guide.zip"
SERVER_VERSION=$(python3 -c 'import json; print(json.load(open("suite-components.json"))["components"]["server"]["version"])')
ACCOUNT_VERSION=$(python3 -c 'import json; print(json.load(open("suite-components.json"))["components"]["account"]["version"])')
mkdir server account
tar -xzf "rustdesk-server-${SERVER_VERSION}-x86_64-unknown-linux-gnu.tar.gz" -C server
tar -xzf "rustdesk-account-server-${ACCOUNT_VERSION}-x86_64-unknown-linux-gnu.tar.gz" -C account
```

替换示例客户端版本。未使用账号功能时，可以不下载账号包、不创建 `account` 目录，并跳过相应命令。

## 地址与端口

准备 `rd.example.com` 指向 hbbs/hbbr 主机，`api.example.com` 指向账号 API 主机；示例域名必须替换为自己的域名。ID/中继使用直接 DNS 解析，不通过只代理 HTTP 的 CDN。两台服务可以在同一主机，也可以分开部署。

| 公网端口 | 协议 | 用途 |
| --- | --- | --- |
| 21115 | TCP | hbbs NAT 检测 |
| 21116 | TCP、UDP | ID 注册、信令、协调 |
| 21117 | TCP | hbbr 中继 |
| 80、443 | TCP | 可选账号 API 的 HTTPS 和证书验证 |

在云安全组、宿主防火墙和路由端口映射上分别放行需要的端口。此原生客户端方案不需要公开 21118/21119 WebSocket 端口，也不把账号 API 的内部 21114 暴露公网。Native P2P 不增加服务端监听端口，但两端客户端必须允许 RustDesk 的出站 UDP 和对应回包；仅开放 hbbs 的 21116 UDP 不能保证任意 NAT 都能直连。

如果 hbbs 改为 `22116`，则 NAT 检测为 `22115`，中继可设为 `22117`；同时修改服务命令、`-r` 公网中继地址、防火墙和客户端填写值。已有部署应保留自己的端口。

## 首次安装 hbbs/hbbr

下面命令从解压后的下载目录执行；适用于尚无这套数据和服务的新部署。Linux 二进制需要 Ubuntu 24.04 或兼容环境，缺少运行库时先安装系统提供的 OpenSSL、Wayland/XKB 运行库并通过 `ldd server/hbbs` 核对。

```sh
sudo useradd --system --home /var/lib/rustdesk-server --shell /usr/sbin/nologin rustdesk-server
sudo install -d -m 0755 /opt/rustdesk-server/bin
sudo install -d -m 0700 -o rustdesk-server -g rustdesk-server /var/lib/rustdesk-server
sudo install -m 0755 server/hbbs server/hbbr server/rustdesk-utils /opt/rustdesk-server/bin/
sudo install -m 0644 res/server-deploy/hbbs.service /etc/systemd/system/rustdesk-gx-hbbs.service
sudo install -m 0644 res/server-deploy/hbbr.service /etc/systemd/system/rustdesk-gx-hbbr.service
sudoedit /etc/systemd/system/rustdesk-gx-hbbs.service
```

在 hbbs 单元的 `ExecStart` 中把 `rd.example.com:21117` 改成客户端能访问的真实中继地址。保留 `NATIVE_P2P=Y`；需要账号登录时保留 `HBBS_ACCOUNT_HANDSHAKE=Y`。确认没有 `ALWAYS_USE_RELAY=Y`，否则不会进入 Native P2P。

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now rustdesk-gx-hbbs.service
sudo systemctl enable --now rustdesk-gx-hbbr.service
sudo systemctl status rustdesk-gx-hbbs.service rustdesk-gx-hbbr.service
sudo cat /var/lib/rustdesk-server/id_ed25519.pub
```

最后一行是**可以填写到客户端的公钥**。`id_ed25519` 是服务器私钥，不能发送给客户端。两个服务共用工作目录和同一组密钥，`-k _` 使用这套密钥校验客户端。不要删除密钥来修复连接问题，换密钥会使已有客户端 Key 失效。

## 可选：部署账号、设备列表与地址簿

账号服务不是官方 Server Pro，也不替代远控密码/被控端确认。需要账号功能时，先安装 Docker Engine 和 Compose 插件，并确认 80/443 未被其他服务占用；已有 HTTPS 反向代理的主机应接入现有代理。

```sh
docker load -i "rustdesk-account-server-${ACCOUNT_VERSION}-docker-linux-x86_64.tar.gz"
sudo install -d -m 0755 /opt/rustdesk-account-server
sudo cp -a account/deploy /opt/rustdesk-account-server/
cd /opt/rustdesk-account-server/deploy
sudo cp .env.example .env
sudoedit .env
```

把 `ACCOUNT_DOMAIN` 改为 `api.example.com` 的真实域名，把 `ACME_EMAIL` 改为自己的邮箱。检查 `172.30.45.0/24` 不与现有网络冲突；若更改网段，需同步修改 Compose 和 `account.toml` 中的精确可信代理地址。

```sh
sudo install -d -m 0700 -o 10001 -g 10001 data secrets caddy-data caddy-config
docker compose config --images
docker compose run --rm --no-deps --volume "$PWD/secrets:/secrets:rw" \
  account --config /etc/account/account.toml init
docker compose run --rm --no-deps account --config /etc/account/account.toml user create myname --generate
docker compose up -d
docker compose ps
curl --fail https://api.example.com/healthz
```

镜像列表应包含 `rustdesk-account-server:<本次账号版本>`；发行包的 Compose 已设置本地镜像且没有 `build`，**不要执行 `docker compose build`**。首次启动 Caddy 需要获取其镜像并申请证书；账号服务镜像由前面的归档导入。生成的账号密码只在交互终端查看和保存，不重定向到日志。`init` 只用于首次建立数据库/主密钥，已有部署不能重跑初始化。

检查公网能访问 HTTPS 健康接口，宿主机没有对外发布 21114，然后按 [客户端配置](client-setup.md) 填写 API 地址。

Linux ARM64：使用对应的原生 hbbs/hbbr 和账号服务包；此版本的账号容器归档只适用于 x86_64。账号包提供 `deploy/systemd/` 原生模板和 `docs/DEPLOYMENT.md`，按自己的路径配置 API 与 HTTPS 代理，不要在 ARM64 上导入 x86_64 镜像。

Windows x64：解压 Windows 服务端 ZIP，在同一个独立数据目录启动 `hbbs.exe -p 21116 -r 真实地址:21117 -k _` 与 `hbbr.exe -p 21117 -k _`；在启动 hbbs 的环境中设置 `NATIVE_P2P=Y`、`HBBS_ACCOUNT_HANDSHAKE=Y`。需要长期运行时用自己的 Windows 服务管理方案固定工作目录、账户和环境变量。示例 Linux systemd 文件不适用于 Windows。macOS 的账号程序可单独运行，但当前 fork 不提供 macOS hbbs/hbbr。

## 验证与排错

1. `journalctl -u rustdesk-gx-hbbs -u rustdesk-gx-hbbr --since '10 minutes ago'` 检查服务状态；用 `ss -lntup` 核对监听端口。
2. 两台客户端使用相同 ID/中继地址和公钥，先确认“就绪”，再连接设备 ID。
3. Native P2P 等待面板应显示本轮诊断；需要立即连接时手动选择“中继连接”。确认两种方式都能进入实际远控画面。
4. 账号登录成功后再次测试远控。出现 `Failed to secure tcp` 时核对 hbbs 是否为配套版本、是否开启 `HBBS_ACCOUNT_HANDSHAKE=Y`、客户端 Key 是否匹配。
5. API 登录失败时检查 HTTPS 证书、API 地址是否误加 `/api`、Caddy 和账号进程状态。不要用 HTTP、忽略证书校验或删除密钥作为修复方式。

## 已有部署升级与回滚

- 从 `suite-components.json` 记录此次客户端、服务端、账号服务及两套 common 提交。只因它们放在同一个 Release，不代表所有服务必须同时重装。
- 升级 hbbs/hbbr 前停止相关服务，保存旧二进制、服务配置及整个工作目录（含数据库、WAL 和密钥），再替换二进制。恢复原工作目录、端口、公钥和环境变量后启动；不要在新目录意外生成新密钥。
- 账号服务先使用其 `backup` 命令创建一致性数据库备份，并单独备份主密钥。升级容器时导入新镜像，再修改**现有 Compose** 的账号镜像版本并重建服务；不要覆盖现有配置或初始化数据。具体命令见账号包 `docs/DEPLOYMENT.md`。
- 升级后测试未登录远控、登录后的远控、P2P 和人工中继。回滚保留配套的旧程序、配置和数据库；账号库的恢复/迁移规则以对应版本文档为准。

端口与客户端基础字段参考 [官方 OSS 安装说明](https://rustdesk.com/docs/en/self-host/rustdesk-server-oss/install/) 和 [官方客户端配置说明](https://rustdesk.com/docs/en/self-host/client-configuration/)。上述 Native P2P、账号握手及联合发布是本 fork 的附加行为。
