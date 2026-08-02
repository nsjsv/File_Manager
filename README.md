# File Manager

File Manager 是一个面向 Linux 桌面的文件管理器，主要面向 Wayland 环境，使用 Rust 编写，UI 基于 Iced。

它的目标不是做一个“大而全”的跨平台文件管理器，而是把 Linux 日常文件操作中最常见、最需要信任感的部分做扎实：路径浏览、搜索、预览、复制移动、回收站、外部设备和键盘操作。

当前项目仍处于早期阶段，功能和体验还会继续调整。下面只介绍项目比较有辨识度的方向，不作为完整功能对照表。

## 特色功能

- **多栏、列表、标签页和分屏并存**
  多栏视图适合沿着目录层级快速移动，列表视图适合扫描和排序。标签页和分屏面板则服务于多个位置之间的复制、移动和对照。

- **更重视可靠性的文件操作**
  复制和移动进入后台队列，支持进度显示、暂停、恢复和取消。遇到同名目标时可以选择替换、跳过、保留两者或合并目录；复制/移动还可以选择关闭校验、基础元数据校验或更强的内容校验。常见文件操作会进入撤销/重做历史，也支持批量重命名以及常见压缩包的创建和解压。

- **围绕索引设计的文件搜索**
  后台搜索服务为 Home 建立并持续维护索引，应用从当前目录发起递归搜索。搜索服务不可用时，本地当前目录搜索仍可回退到目录扫描。

- **预览是工作流的一部分**
  预览窗口覆盖文本、Markdown、目录树、压缩包、图片、音频和视频等常见内容，目标是减少“为了确认一下内容就打开外部应用”的次数。

- **贴近 Linux 桌面环境**
  侧边栏会读取常见用户目录和 GTK 书签，并允许管理收藏；存储设备通过 UDisks2 显示，并提供挂载、卸载和安全移除等动作。还可以连接 SMB、WebDAV 和 SFTP 网络位置。打开终端、默认应用和桌面主题等行为尽量贴近 Linux 桌面习惯。

- **可配置的工作台**
  设置窗口集中管理隐藏文件显示、启动位置与会话恢复、界面语言、终端选择、渲染 GPU 偏好、文件操作校验和快捷键。属性窗口则用于查看文件信息、目录内容统计和权限，并直接修改常见权限位。

## 技术栈

- Rust
- Iced
- Tokio
- notify
- SQLite / rusqlite
- UDisks2
- Rodio
- pulldown-cmark

## 运行依赖

核心文件操作和桌面集成功能依赖以下系统库或命令：

- 打开文件和查询默认应用需要 `xdg-open`、`xdg-mime`；Arch Linux 由 `xdg-utils` 包提供
- Wayland 文件剪贴板需要 `wl-copy`、`wl-paste`；Arch Linux 由 `wl-clipboard` 包提供
- “打开方式”需要 `gio`；Arch Linux 由 `glib2` 包提供
- 文件任务桌面通知需要 `notify-send`；Arch Linux 由 `libnotify` 包提供
- 复制和跨文件系统移动保留 POSIX ACL 需要 `libacl`；Arch Linux 由 `acl` 包提供

可选预览和压缩包工具：

- 视频预览和元数据读取需要 `ffmpeg`、`ffprobe`，视频缩略图也可以使用 `ffmpegthumbnailer`
- 创建 `.7z` 压缩包以及预览、解压 `.7z` 和 `.rar` 文件，需要 `7z`、`7zz` 或 `7za`；Arch Linux 由 `7zip` 包提供

可选桌面服务：

- SFTP 和 WebDAV 连接需要 `gio` 及 GVfs backend；Arch Linux 由 `gvfs` 包提供
- SMB 连接还需要 SMB GVfs backend；Arch Linux 由 `gvfs-smb` 包提供
- MTP Android 和媒体播放器需要 MTP GVfs backend；Arch Linux 由 `gvfs-mtp` 包提供
- 数码相机需要 GPhoto GVfs backend；Arch Linux 由 `gvfs-gphoto2` 包提供
- Apple/AFC 设备需要 AFC GVfs backend；Arch Linux 由 `gvfs-afc` 包提供
- 缺少对应 backend 时，便携设备不会显示为可用 Devices，已有 UDisks 和 Network 位置不受影响
- 保存网络连接密码需要 `secret-tool`；Arch Linux 由 `libsecret` 包提供
- 存储设备发现、挂载和安全移除需要 UDisks2 服务；Arch Linux 由 `udisks2` 包提供

## 安装

Arch Linux 用户可以从 AUR 安装预编译包：

```bash
yay -S file-manager-bin
# 或
paru -S file-manager-bin
```

索引搜索依赖可用的 systemd user manager、统一 cgroup v2，以及 memory 和 cpu controller。安装包会提供 `file-manager-search.service`；可以使用以下命令显式启用、查看状态和日志：

```bash
systemctl --user enable --now file-manager-search.service
systemctl --user status file-manager-search.service
journalctl --user -u file-manager-search.service -f
```

应用内可在“设置 → 日志”查看本次系统启动以来 App 与搜索服务最近 200 条日志，并按 Error、Warning、Info、Debug 调整本次会话的显示级别。日志由 journald 持久化和轮转；应用不会另建日志文件。

环境不满足这些条件时，应用不会绕过 systemd 直接启动 daemon；索引和全局搜索会显示不可用原因，当前目录搜索 fallback 仍可使用。

## 命令行启动

安装后的命令名为 `file-manager`。不传路径时，应用继续使用设置中的 Home、自定义目录或上次会话启动策略：

```bash
file-manager
```

传入本地目录会按参数顺序在同一个窗格中打开多个标签；传入文件会打开其父目录并选中文件。同一父目录下的多个文件会合并到一个多选标签。应用已经运行时，后续 `file-manager [PATH]...` 会复用已有实例：无路径只聚焦主窗口，有路径则复用同目录标签并追加缺失目录，不清空其它标签或分栏。

```bash
file-manager .
file-manager ~/Downloads ~/Documents
file-manager ~/Downloads/report.pdf ~/Downloads/notes.txt
```

路径可以是绝对路径或相对当前终端目录的路径。所有路径会在创建窗口前统一验证，任一路径不可用都会拒绝整次启动。当前 CLI 只接受本地文件系统路径，不接受 URI。

查看帮助和版本：

```bash
file-manager --help
file-manager --version
```

源码运行时参数语义相同；`--` 用于分隔 Cargo 和 App 参数：

```bash
cargo run --locked -p app-ui -- ~/Downloads ~/Documents
cargo run --locked -p app-ui -- ~/Downloads/report.pdf
./target/release/app-ui ~/Downloads
```

## 编译运行

需要先安装 Rust 工具链、系统构建依赖和核心运行命令。Arch Linux 可以使用：

```bash
sudo pacman -S --needed base-devel git rust cargo pkgconf \
  acl alsa-lib fontconfig glib2 libnotify libxkbcommon wayland wl-clipboard xdg-utils
```

按需安装可选预览、压缩包、网络位置、密码和存储设备支持：

```bash
sudo pacman -S --needed 7zip ffmpeg ffmpegthumbnailer \
  gvfs gvfs-afc gvfs-gphoto2 gvfs-mtp gvfs-smb libsecret udisks2
```

克隆仓库并直接运行 App：

```bash
git clone https://github.com/nsjsv/File_Manager
cd File_Manager
cargo run --locked -p app-ui
```

如需同时构建 release App 和搜索 daemon：

```bash
cargo build --release --locked -p app-ui -p file-search
./target/release/app-ui
```

源码构建的 App 二进制名为 `app-ui`，搜索 daemon 二进制名为 `file-searchd`。以上命令不会安装 systemd user unit 或品牌 D-Bus service；没有兼容的已安装搜索服务时，索引搜索不可用，本地当前目录搜索仍可回退到目录扫描。正式发行包会安装 `io.github.nsjsv.FileManager` 品牌激活服务，供 CLI 和桌面组件复用已有实例。

运行中的应用会以 `DoNotQueue` 尝试提供标准 `org.freedesktop.FileManager1` 接口。若 Nautilus、Dolphin、Thunar 等文件管理器已经占用标准名称，本应用不会替换或排队抢占；品牌单实例端点和普通 GUI 仍正常工作。发行包不会安装另一个声明标准名称的 D-Bus service 文件。

## 平台说明

目前主要在 Linux / Wayland 环境下开发和测试。其他发行版如果遇到预编译二进制无法运行，建议先从源码编译。

项目暂时不保证 Windows、macOS 和 X11 环境的可用性。

## AI 辅助提交说明

如果 PR 中的代码主要由 AI 生成，或经过 AI 大量辅助修改，请在 PR 标题中加上 `[AI]` 标识，例如：

```text
[AI] 修复文件搜索刷新问题
```

如果只是用 AI 做少量解释、查资料或辅助排查，可以不加该标识。

## 反馈

欢迎提交 issue 或 PR。早期阶段更希望收到下面这些反馈：

- 哪些操作不符合你的使用习惯
- 哪些发行版或桌面环境下无法运行
- 文件操作、搜索、预览等功能是否稳定
- 哪些功能应该优先补齐

<a href="https://www.star-history.com/">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/chart?repos=nsjsv/File_Manager&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/chart?repos=nsjsv/File_Manager&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/chart?repos=nsjsv/File_Manager&type=date&legend=top-left" />
 </picture>
</a>

## 许可证

GPL-3.0-or-later

## 友链

[LinuxDo](https://linux.do/)
