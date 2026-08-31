# Niri 悬浮窗口配置

File Manager 的设置、属性和预览都是独立窗口。在 [Niri](https://github.com/YaLTeu/niri) 下，这些窗口默认可能被平铺进当前列；加一条 `window-rule` 可以让它们像对话框一样悬浮打开。

## 窗口 app-id

| 窗口 | app-id |
| --- | --- |
| 主窗口 | `file-manager` |
| 设置 | `file-manager-settings` |
| 属性 | `file-manager-properties` |
| 预览 | `file-manager-preview` |

## 添加 window-rule

在 `~/.config/niri/config.kdl` 中，与已有的 `window-rule` 并列加入：

```kdl
window-rule {
    match app-id="file-manager-settings"
    match app-id="file-manager-properties"
    match app-id="file-manager-preview"
    open-floating true
}
```

保存后 Niri 会自动重载配置。

## 说明

- 规则在窗口打开时应用：已打开的窗口不受影响，关掉重开即可按规则浮动；
- 主窗口不在规则里，保持平铺；如果也想悬浮主窗口，可以自行加一行 `match app-id="file-manager"`；
- 可以用 `niri msg windows` 查看当前各窗口的 app-id，用于确认规则是否匹配。
