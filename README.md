# FreeCharles (Tauri)

Charles 风格的桌面 HTTP/HTTPS 抓包代理，由 Electron 版本重构为 **Tauri 2 + Rust + Vue 3**。
界面、交互与功能保持与 Electron 版一致：Structure / Sequence 两种列表视图、请求详情、重放、Compose、会话保存、HAR 导出、Windows 系统代理自动接管（首次生效前请求确认）、浅色/暗黑主题、文件日志与诊断导出，以及可选的 HTTPS 解密（本地 CA 做 TLS 中间人）等。

## 目录结构

```
free_charles/
├─ src/                     # Vue 3 前端（Vite）
│  ├─ components/           # 工具栏、请求列表、详情面板、对话框等
│  ├─ stores/               # 会话状态与轻提示（模块级共享）
│  ├─ lib/api.ts            # 统一的后端调用入口（invoke + 事件订阅 + 系统对话框）
│  ├─ utils/                # cURL、格式化、Structure 树、JSON 高亮
│  └─ assets/styles/        # 全局 SCSS 变量与基础样式
└─ src-tauri/               # Rust 后端
   ├─ src/proxy.rs          # 抓包代理（hyper 1.x）：HTTP 转发、CONNECT 隧道、HTTPS 中间人、Upgrade 透传
   ├─ src/cert.rs           # 本地 HTTPS 解密 CA：根证书生成 / 加载、按域名签发证书、Windows 信任安装
   ├─ src/store.rs          # 会话存储（内存预算、Body 缓存、延迟加载、导入导出）
   ├─ src/body.rs           # gzip / deflate / br / zstd 解码与文本、二进制展示
   ├─ src/logger.rs         # 文件日志、轮转与诊断日志读取
   ├─ src/system_proxy.rs   # Windows 系统代理接管与恢复（注册表备份）
   ├─ src/settings.rs       # settings.json 持久化与范围校验
   ├─ src/har.rs            # HAR 1.2 导出
   ├─ src/commands.rs       # Tauri 命令（等价于原 Electron IPC handler）
   ├─ src/menu.rs           # 原生菜单（文件 / 编辑 / 会话 / 代理 / 视图 / 帮助）
   └─ src/lib.rs            # 应用启动、全局状态、系统代理同步、退出清理
```

## 开发

```bash
npm install          # 安装前端依赖
npm run app:dev      # 启动 Tauri 开发模式（自动拉起 Vite:1420）
npm run dev          # 仅启动前端（浏览器里无法使用 IPC）
npm run test         # 前端单元测试（Vitest）
cd src-tauri && cargo test   # Rust 单元测试
```

## 构建

```bash
npm run app:build            # 生成安装包（Windows NSIS）
npx tauri build --bundles appimage  # Linux AppImage（需在 Linux 环境执行）
npx tauri build --no-bundle  # 只生成可执行文件
npm run build                # 仅前端类型检查 + 打包
```

## 持续集成与发布

`.github/workflows/ci.yml` 在 Windows 上依次执行：安装依赖 → 前端类型检查与打包 → 前端单测 → Rust 单测 → `cargo fmt --check` → `cargo clippy --all-targets -- -D warnings` → `tauri build`（NSIS 安装包）→ 上传构建产物；同时在 Ubuntu 上构建 AppImage。

触发规则：

| 事件 | 行为 |
| --- | --- |
| push 到 `master` / `main` | 构建并上传 artifact（14 天） |
| Pull Request | 同上，用于提前发现编译与格式问题 |
| 手动 `workflow_dispatch` | 同上 |
| push `v*` 标签 | 校验版本号一致后构建，并创建 GitHub Release |

发布流程：

1. 同时更新三处版本号：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`（CI 会校验三者与 tag 一致）。
2. 提交代码并打标签：

   ```bash
   git tag v1.1.0
   git push origin master --tags
   ```

3. Actions 自动构建并发布，Release 中会包含：
   - `FreeCharles_<version>_x64-setup.exe`：NSIS 安装包（当前用户安装）
   - `FreeCharles-portable.exe`：免安装可执行文件
   - `FreeCharles_<version>_amd64.AppImage`：Linux 免安装包

说明：

- 带后缀的标签（如 `v1.1.0-beta.1`）会被标记为 pre-release。
- 未配置代码签名，用户首次运行安装包时 Windows SmartScreen 会提示「未知发布者」。
- 当前构建 Windows x64（NSIS）与 Linux x64（AppImage）；macOS 产物可按需增加对应 job 与系统依赖。
- Linux 下系统代理自动接管与一键安装根证书仍仅 Windows 可用，需要手动配置代理和导入证书。

## 抓包使用说明

1. 启动应用后默认监听 `127.0.0.1:8888`；首次接管 Windows 系统代理前会请求确认，拒绝后可在设置中重新开启。
2. 浏览器或系统把 HTTP 代理指向该地址后，即可看到 HTTP 请求。
3. HTTPS 默认以 CONNECT 隧道形式透传（不解密），列表中只显示隧道记录。
4. 停止代理或退出应用会自动恢复原来的系统代理设置；异常退出后下次启动也会自动恢复。

## 开启 HTTPS 解密

1. 打开 **设置 → 捕获行为 → HTTPS 解密根证书**，点击「生成根证书」。
2. 点击「安装/信任根证书」，把本地根证书导入 **当前用户 → 受信任的根证书颁发机构**（仅 Windows 支持一键安装；其他平台请手动导入 `freecharles-ca.crt`）。
3. 打开「解密 HTTPS 内容」开关并保存；重新访问 HTTPS 站点即可看到解密后的明文请求。
4. 解密后的条目在列表中显示锁图标，详情面板带「HTTPS 解密」标识，可正常查看请求/响应头、Body、时序，也支持重放。

注意事项：

- 根证书与私钥只保存在本机（`%APPDATA%\com.freecharles.app\https-ca\`），不会上传或发送给渲染进程。
- 仅在你有权抓包的设备和流量上使用；抓包结束后建议在系统证书管理器中移除该根证书。
- 「忽略上游 HTTPS 证书错误」只用于自签名 / 内部证书的上游服务，开启后代理到目标服务器不再校验证书，存在安全风险。
- 若站点启用 HTTP/2 / HTTP/3，代理仅协商 HTTP/1.1；证书固定（pinning）或双向认证（mTLS）的客户端无法解密。

## 与 Electron 版本的差异

- 请求头名称统一按小写记录（hyper 不保留原始大小写），影响仅限于 Raw 视图展示。
- 解密证书使用 ECDSA P-256（Electron 版为 RSA 2048），两者都被主流浏览器与系统信任存储接受。
- 大 Body 在详情中延迟加载；会话内容总内存预算约 256 MB，超出后优先淘汰最旧记录。
- 单次解压输出上限为 128 MB，避免压缩炸弹耗尽内存。

## 主题与日志

- 工具栏右侧的日/月图标可切换浅色/暗黑主题，选择会持久化到 `settings.json`。
- 启动、代理启停、系统代理接管和运行期错误会写入应用数据目录下的 `logs/main.log`，超过 2 MB 自动轮转为 `main.1.log`。
- **帮助 → 打开日志目录** 可直接定位日志；**帮助 → 导出诊断信息…** 会生成包含版本、系统信息、代理状态、设置和最近日志的文本文件。
