# File Manager

File Manager 是一个面向 Linux 桌面的文件管理器，主要面向 Wayland 环境，使用 Rust 编写，UI 基于 Iced。

它的目标不是做一个“大而全”的跨平台文件管理器，而是把 Linux 日常文件操作中最常见、最需要信任感的部分做扎实：路径浏览、搜索、预览、复制移动、回收站、外部设备和键盘操作。

当前项目仍处于早期阶段，功能和体验还会继续调整。下面只介绍项目比较有辨识度的方向，不作为完整功能对照表。

## 特色功能

- **多栏、列表、标签页和分屏并存**
  多栏视图适合沿着目录层级快速移动，列表视图适合扫描和排序。标签页和分屏面板则服务于多个位置之间的复制、移动和对照。

- **更重视可靠性的文件操作**
  复制和移动进入后台队列，支持进度显示、暂停、恢复和取消。遇到同名目标时可以选择替换、跳过、保留两者或合并目录；复制/移动还可以选择关闭校验、基础元数据校验或更强的内容校验。常见文件操作会进入撤销/重做历史。

- **围绕索引设计的文件搜索**
  搜索不是临时扫一遍目录就结束。首次启动可以选择要建立索引的位置，索引任务进入同一套后台队列；搜索窗口可以在当前目录和 Home 范围之间快速切换。

- **预览是工作流的一部分**
  预览窗口覆盖文本、Markdown、目录树、压缩包、图片、音频和视频等常见内容，目标是减少“为了确认一下内容就打开外部应用”的次数。

- **贴近 Linux 桌面环境**
  侧边栏会读取常见用户目录和 GTK 书签，也会通过 UDisks2 显示存储设备，并提供挂载、卸载和安全移除等动作。打开终端、默认应用和桌面主题等行为尽量贴近 Linux 桌面习惯。

- **可配置的工作台**
  设置窗口集中管理隐藏文件显示、终端选择、渲染 GPU 偏好、文件操作校验和快捷键。属性窗口则用于查看文件信息、目录内容统计和权限，并直接修改常见权限位。

## 还缺少什么

这个项目还没有到“可以替代所有系统文件管理器”的阶段。比较重要的缺口包括：

- **搜索索引管理**：索引排除规则、增量维护、重建入口、索引状态可视化还需要继续完善。
- **更成熟的文件操作审计**：后台队列和撤销/重做已经有基础，但跨会话历史、失败重试、详细日志和更完整的恢复策略仍需加强。
- **国际化和无障碍**：当前界面文案主要是英文，键盘导航、屏幕阅读器和多语言支持还没有作为完整体系收束。

## 技术栈

- Rust
- Iced
- Tokio
- nucleo
- notify
- SQLite / rusqlite
- UDisks2
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
