# 个人账号服务的客户端传输保护

账号登录令牌和 API 密码需要保密，不能使用兼容旧服务端时允许的明文降级。hbbs 支持加密握手后，客户端仍须在发送凭据前确认加密成功；API 的 HTTPS 不覆盖独立的信令连接。

本修复只改动两个已有源文件：

- `src/client.rs`：打洞、中继申请、账号保活携带账号令牌时，必须完成 TCP 加密握手，或使用 WSS。缺少有效服务器公钥、收到其他首包或使用明文 WebSocket 时拒绝发送。已完成 WebRTC 信令握手的连接复用现有加密状态。未携带令牌的原连接路径保留。
- `src/common.rs`：仅供 TCP API 代理使用的 `secure_tcp_silent` 检查加密结果，防止 HTTPS 失败后的备用请求把登录密码或令牌发到明文连接。通用 `secure_tcp` 的旧服务端兼容行为保留。

没有修改登录界面、账号协议、P2P 协调格式或共享库接口。合并上游时检查这两个发送入口是否仍强制确认加密；若上游已提供等价保护，优先删除本地补丁。

回归命令（Windows，具备现有 Flutter/编解码构建依赖）：

```sh
cargo test --locked --release --lib --features flutter,hwcodec -- --test-threads=1 test_secure_tcp account_tokens_are_not_sent
```

测试使用回环服务端和一次性令牌，覆盖非握手首包、缺失公钥、连接关闭、正常协商以及签名版本验证。此测试不替代两端 GUI、Android 真机或完整远控验收。

需要更新实际安装的客户端才能获得保护；仅推送源码或更新 hbbs 不会修复旧客户端。服务器握手开关与密钥配置参见配套 rustdesk-server 的 `docs/account-handshake.md`。
