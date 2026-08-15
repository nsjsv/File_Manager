# Matugen 配色

File Manager 会读取 `$XDG_CONFIG_HOME/file-manager/matugen.toml`（默认 `~/.config/file-manager/matugen.toml`），并在文件变化时热更新所有已打开窗口。Matugen 是可选依赖；没有生成文件时，应用继续使用系统深浅色主题。

发行包中的模板位于：

```text
/usr/share/file-manager/matugen/file-manager-colors.toml
```

源码仓库中的模板位于 `packaging/matugen/file-manager-colors.toml`。把下面内容合并到 `~/.config/matugen/config.toml`；已有 `[config]` 时不要重复添加该表：

```toml
[config]

[templates.file-manager]
input_path = "/usr/share/file-manager/matugen/file-manager-colors.toml"
output_path = "~/.config/file-manager/matugen.toml"
```

## DMS 自动更新

DMS 会把 `~/.config/matugen/config.toml` 中的用户模板合并进每次主题生成。确认 DMS 的 **Run user Matugen templates** 已启用后，上面的 `templates.file-manager` 会在切换壁纸或深浅模式时自动生成 File Manager 配色；不需要 `post_hook`。

源码调试且尚未安装发行包时，可把 `input_path` 改为仓库模板的绝对路径，或先复制模板：

```bash
install -Dm644 packaging/matugen/file-manager-colors.toml \
  ~/.config/matugen/templates/file-manager-colors.toml
```

然后使用：

```toml
[templates.file-manager]
input_path = "~/.config/matugen/templates/file-manager-colors.toml"
output_path = "~/.config/file-manager/matugen.toml"
```

## 直接使用 Matugen

创建输出目录后，按桌面模式生成配色：

```bash
mkdir -p ~/.config/file-manager
matugen image ~/Pictures/wallpaper.jpg -m dark
matugen image ~/Pictures/wallpaper.jpg -m light
```

不需要配置 `post_hook`。有效输出会直接替换当前配色；删除输出文件会恢复系统主题；短暂或畸形写入会被忽略并保留上一套有效配色。
