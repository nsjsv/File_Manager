# File Manager

File Manager 是一个面向 Linux 桌面的文件管理器。

项目目前主要面向 Wayland 环境，使用 Rust 编写，UI 基于 Iced。目标是做一个交互直接、适合日常使用的 Linux 原生文件管理器。

当前项目仍处于早期阶段，功能和体验还会继续调整。

## 已有功能

- 目录浏览
- 文件和文件夹打开
- 多标签页
- 多栏视图
- 文件复制、移动、重命名
- 新建文件和文件夹
- 删除到回收站
- 回收站浏览、恢复、清空
- 显示或隐藏隐藏文件
- 按名称、大小、类型、修改时间排序
- 图片和视频缩略图
- 文件预览窗口
- 文本和 Markdown 预览
- 目录和压缩包树形预览
- 图片预览
- 音频预览和播放控制
- 视频预览和播放控制
- 文件搜索
- 目录变化监听和自动刷新
- 后台文件操作进度显示、暂停、恢复和取消
- 需要终端运行的默认应用打开支持

## 技术栈

- Rust
- Iced
- Tokio
- Tantivy
- notify
- SQLite
- Rodio
- pulldown-cmark

## 可选运行依赖

部分预览功能会调用系统工具：

- 视频预览和视频缩略图建议安装 `ffmpeg` 和 `ffprobe`，视频缩略图也可以使用 `ffmpegthumbnailer`
- `.7z` 和 `.rar` 压缩包预览需要安装 `7z`、`7zz` 或 `7za`

## 安装

Arch Linux 用户可以从 AUR 安装预编译包：

```bash
yay -S file-manager-bin
# 或
paru -S file-manager-bin
```

## 编译运行

需要先安装 Rust 工具链。

```bash
git clone https://github.com/nsjsv/File_Manager
cd File_Manager
cargo run -p app-ui
```

构建 release 版本：

```bash
cargo build --release -p app-ui
./target/release/app-ui
```

## 本地安装

可以把 release 二进制安装到用户目录：

```bash
cargo build --release -p app-ui
install -Dm755 target/release/app-ui ~/.local/bin/file-manager
```

然后运行：

```bash
file-manager
```

如果需要桌面启动项，可以创建 `~/.local/share/applications/file-manager.desktop`：

```desktop
[Desktop Entry]
Type=Application
Name=File Manager
Exec=file-manager
Categories=System;FileManager;
Terminal=false
```

## 维护者发布

发布新版本时：

1. 更新 Cargo 工作区版本，确保 `app-ui` 包版本等于准备发布的 `X.Y.Z`。
2. 推送形如 `vX.Y.Z` 的 tag，例如 `git tag vX.Y.Z && git push origin vX.Y.Z`。
3. 在 GitHub 仓库配置 AUR 发布所需 Secrets：
   - `AUR_SSH_PRIVATE_KEY`：可推送 `file-manager-bin` AUR 仓库的 SSH 私钥。
   - `AUR_KNOWN_HOSTS`：AUR SSH 主机的 known_hosts 记录。
   - `AUR_COMMIT_NAME`、`AUR_COMMIT_EMAIL`：可选，缺省使用 `github-actions[bot]`。
4. release workflow 会先构建并发布 GitHub Release tarball，然后仅在 tag push 事件中渲染 `PKGBUILD`、生成 `.SRCINFO` 并推送到 AUR。

`AUR_SSH_PRIVATE_KEY` 对应的公钥需要添加到 AUR 账户，并且该私钥应能在 CI 中非交互使用。`AUR_KNOWN_HOSTS` 可以用下面的命令生成，写入前请按 AUR 官方信息核对 fingerprint：

```bash
ssh-keyscan aur.archlinux.org
```

发 tag 前请确认 release workflow、`packaging/aur/file-manager-bin/PKGBUILD.in` 和 `packaging/linux/file-manager.desktop` 都已经提交到 tag 指向的 commit。

release workflow 只由 `vX.Y.Z` tag push 触发，不提供手动触发入口。

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

## 许可证

GPL 3.0

## 友链

[LinuxDo](https://linux.do/)
