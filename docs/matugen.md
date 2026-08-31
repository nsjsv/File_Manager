# Matugen 配色

[Matugen](https://github.com/InioX/matugen) 可以从壁纸生成 Material 风格配色。File Manager 支持读取 Matugen 生成的配色文件，壁纸换色调，文件管理器跟着换。

## 工作方式

应用会读取 `$XDG_CONFIG_HOME/file-manager/matugen.toml`（默认 `~/.config/file-manager/matugen.toml`），并在文件变化时热更新所有已打开的窗口，不需要重启。

Matugen 是可选依赖：没有生成文件时，应用继续使用系统深浅色主题。

配色只在你于 **设置 → 外观 → 配色家族** 中选择 Matugen 后生效。选中后，深浅模式由生成文件自身的 `mode` 决定，外观设置里的深浅模式控件不可用；切回内置配色家族后恢复。

## 启用步骤

1. 在设置的外观配色家族中选择 **Matugen**；
2. 把下面的模板配置合并进 `~/.config/matugen/config.toml`（已有 `[config]` 时不要重复添加该表）：

```toml
[config]

[templates.file-manager]
input_path = "/usr/share/file-manager/matugen/file-manager-colors.toml"
output_path = "~/.config/file-manager/matugen.toml"
```

3. 生成一次配色：

```bash
matugen image ~/Pictures/wallpaper.jpg -m dark
```

### 从源码构建

尚未安装发行包时，发行包模板路径不存在，可以先复制模板再引用：

```bash
install -Dm644 packaging/matugen/file-manager-colors.toml \
  ~/.config/matugen/templates/file-manager-colors.toml
```

```toml
[templates.file-manager]
input_path = "~/.config/matugen/templates/file-manager-colors.toml"
output_path = "~/.config/file-manager/matugen.toml"
```

## 配合 DMS 自动更新

DMS 会把 `~/.config/matugen/config.toml` 中的用户模板合并进每次主题生成。确认 DMS 的 **Run user Matugen templates** 已启用后，上面的 `templates.file-manager` 会在切换壁纸或深浅模式时自动生成 File Manager 配色，不需要配置 `post_hook`。

## 直接使用 Matugen

不想改 Matugen 配置的话，创建输出目录后按桌面模式手动生成也可以：

```bash
mkdir -p ~/.config/file-manager
matugen image ~/Pictures/wallpaper.jpg -m dark
matugen image ~/Pictures/wallpaper.jpg -m light
```

## 行为说明

- 不需要配置 `post_hook`，有效输出会直接替换当前配色；
- 删除输出文件会恢复系统主题；
- 短暂或畸形的写入会被忽略，并保留上一套有效配色；
- 文件缺失时即使选中了 Matugen，也会回退到当前系统主题；
- 未选择 Matugen 时，文件更新只会更新缓存，当前界面不换色。

模板会导出应用使用的全部 Material 颜色角色，见 `/usr/share/file-manager/matugen/file-manager-colors.toml`（源码仓库中为 `packaging/matugen/file-manager-colors.toml`）。
