# RustDesk GX 发布流程

本 fork 的发布仓库是 `GX-Swjtu/rustdesk`。合并上游和正式发布均由维护者手动决定；工作流不会定时合并上游，不会自动发布正式版本，也不会部署任何服务器。

## 客户端更新规则

- 客户端检查官方 `rustdesk/rustdesk` 和本 fork 的 GitHub 最新正式 Release。草稿、预发布、nightly 和只有 Git tag 的版本不算正式更新。
- 官方新版只产生通知。比较基线来自 `res/fork-upstream-version`，表示已经手动合并的官方稳定标签；不能用上游 master 中提前递增的 Cargo 版本代替这项基线。
- fork 按 `(major, minor, patch, gx revision)` 比较，因此 `gx.10` 新于 `gx.9`。只有版本更高、含构建完成标记 `fork-release.json`，并有当前平台安装包的正式 Release 才出现下载按钮。
- 下载按钮打开 `https://github.com/GX-Swjtu/rustdesk/releases/tag/<version>`。不跳转官方安装包，不调用原版后台自动安装器。
- 无网络或 GitHub API 限流时提示检查暂不可用，不把错误解释为已经是最新版。检查遵循原客户端的“启动时检查更新”设置。
- `fork-updates` 是本 fork 默认 Cargo feature；关闭它可恢复官方更新路径。开发工作区显示 `1.5.1-gx.0`，发布工作流在临时构建目录写入实际 tag。

## 一次发布

1. 在开发分支手动合并需要的官方稳定 tag，解决冲突并测试。将 `res/fork-upstream-version` 改为所合并的稳定版本，例如 `1.5.0`。工作流会验证该官方 tag 确实是发布提交的祖先。
2. 决定 fork tag：`Cargo.toml` 的版本加 `-gx.N`，例如 `1.5.1-gx.1`。同一基线每次递增 N，不移动或覆盖发布过的 tag。当前源码已是上游开发版 `1.5.1`，已合并的稳定基线为 `1.5.0`；这两个版本会分别记载，不宣称官方已经发布 1.5.1。
3. 提交本次改动，推送分支后创建并推送 tag：

   ```sh
   git tag -a 1.5.1-gx.1 -m "RustDesk GX 1.5.1-gx.1"
   git push origin 1.5.1-gx.1
   ```

4. 在 Actions 查看 **Fork release (all platforms, draft only)**。首次构建需要下载并编译大量原生依赖。失败时修复源码并选择新 tag；仅临时网络故障可重新运行同一个失败作业。手动重新运行 workflow 时必须选择原 tag。
5. 全部成功后，在 Releases 审核草稿的中英文防诈骗提醒、安装包和 SHA256SUMS。工作流会把草稿从预发布状态改为正式发布候选，但仍保持 Draft。
6. 手动点击 **Publish release**，并设为 **Latest**，客户端随后检查时才会出现此版本的下载入口。不得在构建完成前发布草稿。

## 平台和版本

复用上游 Windows x64/ARM64 EXE、MSI 和 x86 Sciter、macOS Intel/Apple Silicon DMG、Linux x64/ARM64 的 DEB/RPM/AppImage/Flatpak、Linux Sciter ARMv7/x64、Arch 和 Android ARM64/ARMv7/x64/Universal 构建。上游可选 DRM 包一并构建。

iOS 生成未签名 `.xcarchive.zip`，需要维护者自己的 Apple 分发签名才能安装，因此客户端不把它当作 iOS 可安装更新。macOS 当前生成未公证 DMG，Windows 当前没有 Authenticode 签名。官方 Web 和 App Store 链接是官方服务，不能冒充本 fork 的发布；此源码中的旧 Web 构建被上游禁用，保持该状态。

显示版本和发布资产名使用完整 fork tag。保留 Cargo 的三段版本用于协议兼容和原生版本资源；Windows MSI、macOS/Windows Flutter build number 使用 fork revision；Android versionCode 使用递增编码；DEB、RPM 和 Arch 包修订号也递增。`N` 范围为 1–999。客户端更新比较不依赖 Cargo/SemVer 将后缀视为 prerelease 的规则。

## 签名与权限

GitHub Actions 只需自动提供的 `GITHUB_TOKEN`，权限 `contents: write`，不需要个人访问令牌。Android 要求以下四个仓库 Secrets，缺任一个会在构建前失败，不能发布临时 debug 签名包：

| Secret | 内容 |
| --- | --- |
| ANDROID_SIGNING_KEY | PKCS12/JKS 文件的 Base64 |
| ANDROID_ALIAS | 固定密钥别名 |
| ANDROID_KEY_STORE_PASSWORD | 密钥库密码 |
| ANDROID_KEY_PASSWORD | 私钥密码 |

签名密钥和密码不得提交 Git。首次切换官方或本地测试 APK 可能因签名不同需要卸载，必须先备份设置。以后全部 fork 发布保持同一签名。发布工作流只生成安装文件和草稿，绝不升级运行中的客户端、hbbs、hbbr、账号服务或数据库。

## 维护与检查

- `python3 -m unittest discover -s .github/scripts -p test_fork_release.py` 检查版本、原生版本注入和发布门槛。
- `cargo test --locked --release --lib --features flutter,hwcodec fork_update::tests` 检查更新来源、版本比较、草稿/缺包/非本 fork URL 拒绝规则。
- `.github/scripts/fork_release.py prepare` 只允许在 CI 临时 checkout 中运行；它会修改构建文件，不用于日常开发目录。
- 上游改变安装包名称、平台矩阵或打包脚本时，更新 `required_assets` 和对应准备逻辑。发布检查会在不匹配时拒绝补上完成标记。
- 完成标记在所有平台构建和校验文件生成后最后上传。工作流失败只留下草稿；客户端不会拿失败草稿更新。

参考：[官方发布页](https://github.com/rustdesk/rustdesk/releases)、[GitHub Releases API](https://docs.github.com/en/rest/releases/releases)、[复用工作流](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows)。
