pub(super) fn translate(text: &str) -> Option<String> {
    let exact = match text {
        "Search - File Manager" => Some("搜索 - 文件管理器"),
        "Search" => Some("搜索"),
        "Searching..." => Some("正在搜索..."),
        "No search results" => Some("没有搜索结果"),
        "Any type" => Some("任意类型"),
        "Folders" => Some("文件夹"),
        "Links" => Some("链接"),
        "Any content" => Some("任意内容"),
        "Images" => Some("图片"),
        "Archives" => Some("压缩包"),
        "Any time" => Some("任意时间"),
        "Today" => Some("今天"),
        "Past 7 days" => Some("近 7 天"),
        "Past 30 days" => Some("近 30 天"),
        "This year" => Some("今年"),
        "Clear search text" => Some("清空搜索文字"),
        "Close search" => Some("关闭搜索"),
        "Open folder" | "Open Containing Folder" => Some("打开所在目录"),
        "Showing the first 100 results" => Some("仅显示前 100 条结果"),
        "Search is available from a local folder" => Some("请从本地文件夹开始搜索"),
        "Search is unavailable for remote folders" => Some("远程文件夹暂不支持搜索"),
        "The result has no containing directory" => Some("该结果没有所在目录"),
        "Search Service" => Some("搜索服务"),
        "Service and Index" => Some("服务与索引"),
        "Search endpoint connected" => Some("搜索端点已连接"),
        "Search endpoint is starting…" => Some("搜索端点正在启动…"),
        "Service: starting" => Some("服务：正在启动"),
        "Service: ready" => Some("服务：就绪"),
        "Service: shutting down" => Some("服务：正在关闭"),
        "Indexed queries: available" => Some("索引查询：可用"),
        "Index File Contents" => Some("索引文件内容"),
        _ => None,
    };
    if let Some(translated) = exact {
        return Some(translated.to_owned());
    }

    if let Some(path) = text.strip_prefix("Search in ") {
        return Some(format!("搜索范围：{path}"));
    }
    if let Some(path) = text.strip_prefix("Search root is unavailable: ") {
        return Some(format!("搜索根目录不可用：{path}"));
    }
    if let Some(count) = text
        .strip_prefix("Partial results after inspecting ")
        .and_then(|body| body.strip_suffix(" entries"))
    {
        return Some(format!("部分结果，已检查 {count} 个条目"));
    }
    if let Some(path) = text.strip_prefix("Containing directory is unavailable: ") {
        return Some(format!("所在目录不可用：{path}"));
    }
    if let Some(error) = text.strip_prefix("Search endpoint unavailable: ") {
        return Some(format!("搜索端点不可用：{error}"));
    }
    if let Some(error) = text.strip_prefix("Indexed queries unavailable: ") {
        return Some(format!("索引查询不可用：{error}"));
    }
    None
}
