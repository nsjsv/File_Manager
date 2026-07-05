use std::borrow::Cow;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::config::UiLanguage;

static CURRENT_LANGUAGE: AtomicU8 = AtomicU8::new(UiLanguage::English.as_u8());

pub(crate) fn set_current_language(language: UiLanguage) {
    CURRENT_LANGUAGE.store(language.as_u8(), Ordering::Relaxed);
}

pub(crate) fn current_language() -> UiLanguage {
    UiLanguage::from_u8(CURRENT_LANGUAGE.load(Ordering::Relaxed))
}

pub(crate) fn current_language_is_chinese() -> bool {
    current_language() == UiLanguage::Chinese
}

pub(crate) fn translate_current(text: &str) -> String {
    translate(current_language(), text).into_owned()
}

pub(crate) fn translate<'a>(language: UiLanguage, text: &'a str) -> Cow<'a, str> {
    if language == UiLanguage::English {
        return Cow::Borrowed(text);
    }

    if let Some(translated) = exact_translation(text) {
        return Cow::Borrowed(translated);
    }

    if let Some(translated) = dynamic_translation(text) {
        return Cow::Owned(translated);
    }

    Cow::Borrowed(text)
}

pub(crate) fn detect_system_language() -> UiLanguage {
    detect_system_language_with(|key| std::env::var(key).ok())
}

fn detect_system_language_with<F>(lookup: F) -> UiLanguage
where
    F: Fn(&str) -> Option<String>,
{
    for key in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
        let Some(value) = lookup(key) else {
            continue;
        };
        if locale_value_is_chinese(&value) {
            return UiLanguage::Chinese;
        }
    }
    UiLanguage::English
}

fn locale_value_is_chinese(value: &str) -> bool {
    value
        .split(':')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            segment
                .split(['.', '@'])
                .next()
                .unwrap_or(segment)
                .to_ascii_lowercase()
        })
        .any(|segment| segment == "zh" || segment.starts_with("zh_"))
}

fn dynamic_translation(text: &str) -> Option<String> {
    if let Some((shown, total)) =
        parse_two_counts(text, "Only showing ", " lines. Full line count: ", ".")
    {
        return Some(format!("当前仅显示 {shown} 行，完整行数为 {total}。"));
    }
    if let Some(count) = parse_prefixed_count(text, "", " tasks") {
        return Some(format!("{count} 个任务"));
    }
    if let Some(count) = parse_prefixed_count(text, "", " items") {
        return Some(format!("{count} 个项目"));
    }
    if let Some(count) = parse_prefixed_count(text, "", " explicit path(s)") {
        return Some(format!("{count} 个显式路径"));
    }
    if let Some(error) = text.strip_prefix("Profile error: ") {
        return Some(format!("配置错误：{error}"));
    }
    if let Some(error) = text.strip_prefix("Index error: ") {
        return Some(format!("索引错误：{error}"));
    }
    if let Some(error) = text.strip_prefix("Unavailable: ") {
        return Some(format!("不可用：{error}"));
    }
    if let Some(error) = text.strip_prefix("Could not load: ") {
        return Some(format!("无法加载：{error}"));
    }
    if let Some(error) = text.strip_prefix("Could not load more text preview: ") {
        return Some(format!("无法继续加载文本预览：{error}"));
    }
    if let Some(error) = text.strip_prefix("Default open failed: ") {
        return Some(format!("默认打开失败：{error}"));
    }
    if let Some(error) = text.strip_prefix("Could not update permissions: ") {
        return Some(format!("无法更新权限：{error}"));
    }
    if let Some(error) = text.strip_prefix("Audio unavailable: ") {
        return Some(format!("音频不可用：{error}"));
    }
    if let Some(error) = text.strip_prefix("Could not load saved network password: ") {
        return Some(format!("无法加载已保存的网络密码：{error}"));
    }
    if let Some(error) = text.strip_prefix("Could not remove saved network password: ") {
        return Some(format!("无法移除已保存的网络密码：{error}"));
    }
    if let Some(error) = text.strip_prefix("Could not connect network location: ") {
        return Some(format!("无法连接网络位置：{error}"));
    }
    if let Some(error) = text.strip_prefix("Could not disconnect network location: ") {
        return Some(format!("无法断开网络位置：{error}"));
    }
    if let Some(error) = text.strip_prefix("File operation queue storage failed: ") {
        return Some(format!("文件操作队列存储失败：{error}"));
    }
    if let Some(error) = text.strip_prefix("Failed to initialize file operation queue storage: ") {
        return Some(format!("初始化文件操作队列存储失败：{error}"));
    }
    if let Some(path) = text.strip_prefix("Original: ") {
        return Some(format!("原始位置：{path}"));
    }
    if let Some(path) = text.strip_prefix("Directory: ") {
        return Some(format!("目录：{path}"));
    }

    None
}

fn parse_prefixed_count(text: &str, prefix: &str, suffix: &str) -> Option<usize> {
    text.strip_prefix(prefix)?
        .strip_suffix(suffix)?
        .parse()
        .ok()
}

fn parse_two_counts(
    text: &str,
    prefix: &str,
    middle: &str,
    suffix: &str,
) -> Option<(usize, usize)> {
    let content = text.strip_prefix(prefix)?.strip_suffix(suffix)?;
    let (left, right) = content.split_once(middle)?;
    Some((left.parse().ok()?, right.parse().ok()?))
}

fn exact_translation(text: &str) -> Option<&'static str> {
    match text {
        "File Manager" => Some("文件管理器"),
        "Search - File Manager" => Some("搜索 - 文件管理器"),
        "Settings - File Manager" => Some("设置 - 文件管理器"),
        "Properties - File Manager" => Some("属性 - 文件管理器"),
        "Preview - File Manager" => Some("预览 - 文件管理器"),
        "Closing window..." => Some("正在关闭窗口..."),
        "Settings" => Some("设置"),
        "General" => Some("通用"),
        "File display" => Some("文件显示"),
        "Startup" => Some("启动"),
        "Terminal" => Some("终端"),
        "File Operations" => Some("文件操作"),
        "Verification" => Some("校验"),
        "Rendering" => Some("渲染"),
        "Shortcuts" => Some("快捷键"),
        "Show Hidden Files" => Some("显示隐藏文件"),
        "Show Recursive Folder Size In List View" => Some("在列表视图中显示文件夹递归大小"),
        "Home directory" => Some("主目录"),
        "Open your home directory on startup." => Some("启动时打开主目录。"),
        "Custom directory" => Some("自定义目录"),
        "Open the configured directory on startup." => Some("启动时打开配置的目录。"),
        "Previous state" => Some("上次状态"),
        "Start in the state from the last close, preserving views and directories." => {
            Some("从上次关闭时的状态启动，并保留视图和目录。")
        }
        "Directory" => Some("目录"),
        "Save" => Some("保存"),
        "Custom Startup Directory" => Some("自定义启动目录"),
        "Language" => Some("语言"),
        "Follow the system language until you choose a manual override." => {
            Some("默认跟随系统语言，直到你手动覆盖。")
        }
        "Auto" => Some("自动"),
        "Use the detected system language." => Some("使用检测到的系统语言。"),
        "English" => Some("English"),
        "Always show the interface in English." => Some("始终以英文显示界面。"),
        "中文" => Some("中文"),
        "Always show the interface in Chinese." => Some("始终以中文显示界面。"),
        "Archive name" => Some("归档名称"),
        "Optional password" => Some("可选密码"),
        "Enter a directory path." => Some("请输入目录路径。"),
        "Choose an existing directory." => Some("请选择一个已存在的目录。"),
        "Enter a whole number of MiB greater than 0." => Some("请输入大于 0 的 MiB 整数。"),
        "Enter a smaller preview size." => Some("请输入更小的预览大小。"),
        "Search" => Some("搜索"),
        "Search files" => Some("搜索文件"),
        "No roots selected" => Some("未选择根路径"),
        "No roots" => Some("没有根路径"),
        "Running" => Some("运行中"),
        "Never" => Some("从未"),
        "Search window is closed" => Some("搜索窗口已关闭"),
        "Building the index in the background. You can search by file name now" => {
            Some("正在后台构建索引。现在你仍可按文件名搜索。")
        }
        "Type to search. Press Tab to switch between current folder and Home" => {
            Some("输入即可搜索。按 Tab 在当前文件夹和主目录之间切换。")
        }
        "Building index. Search will run automatically when ready..." => {
            Some("正在构建索引，准备就绪后会自动开始搜索...")
        }
        "Searching..." => Some("搜索中..."),
        "No matches" => Some("没有匹配项"),
        "Files" => Some("文件"),
        "Contents" => Some("内容"),
        "Media" => Some("媒体"),
        "All" => Some("全部"),
        "Current Folder" => Some("当前文件夹"),
        "Home" => Some("主目录"),
        "Image" => Some("图像"),
        "Audio" => Some("音频"),
        "Video" => Some("视频"),
        "Tasks" => Some("任务"),
        "No tasks" => Some("没有任务"),
        "Pending" => Some("等待中"),
        "Paused" => Some("已暂停"),
        "Canceling" => Some("取消中"),
        "Failed" => Some("失败"),
        "Completed" => Some("已完成"),
        "Canceled" => Some("已取消"),
        "Resume" => Some("继续"),
        "Pause" => Some("暂停"),
        "Cancel" => Some("取消"),
        "Rename" => Some("重命名"),
        "Batch Rename" => Some("批量重命名"),
        "New Folder" => Some("新建文件夹"),
        "New File" => Some("新建文件"),
        "Move to Trash" => Some("移到回收站"),
        "Restore" => Some("恢复"),
        "Delete Permanently" => Some("永久删除"),
        "Empty Trash" => Some("清空回收站"),
        "Copy" => Some("复制"),
        "Move" => Some("移动"),
        "Create Archive" => Some("创建归档"),
        "Extract Archive" => Some("解压归档"),
        "Build Search Index" => Some("构建搜索索引"),
        "Trash" => Some("回收站"),
        "Path" => Some("路径"),
        "File name" => Some("文件名"),
        "New name" => Some("新名称"),
        "Apply this action to all files and folders" => Some("将此操作应用到所有文件和文件夹"),
        "Delete Permanently?" => Some("要永久删除吗？"),
        "Empty Trash?" => Some("要清空回收站吗？"),
        "Drop Files" => Some("拖放文件"),
        "Move files into the current folder." => Some("将文件移动到当前文件夹。"),
        "Copy files into the current folder." => Some("将文件复制到当前文件夹。"),
        "Open With" => Some("打开方式"),
        "Loading applications..." => Some("正在加载应用程序..."),
        "Set as default application" => Some("设为默认应用程序"),
        "Required" => Some("必需"),
        "Hide" => Some("隐藏"),
        "Show" => Some("显示"),
        "Open" => Some("打开"),
        "Open with" => Some("打开方式"),
        "Paste" => Some("粘贴"),
        "Create Archive..." => Some("创建归档..."),
        "Batch Rename..." => Some("批量重命名..."),
        "Delete" => Some("删除"),
        "Add Network Connection" => Some("添加网络连接"),
        "Edit Network Connection" => Some("编辑网络连接"),
        "Connect Network Location" => Some("连接网络位置"),
        "Name" => Some("名称"),
        "Modified Time" => Some("修改时间"),
        "Kind" => Some("类型"),
        "Accessed Time" => Some("访问时间"),
        "Created Time" => Some("创建时间"),
        "URI" => Some("URI"),
        "Username" => Some("用户名"),
        "Password" => Some("密码"),
        "Connect on startup" => Some("启动时连接"),
        "Connect" => Some("连接"),
        "Save & Connect" => Some("保存并连接"),
        "Username is required when a password is provided" => Some("提供密码时必须填写用户名"),
        "WebDAV" => Some("WebDAV"),
        "SFTP" => Some("SFTP"),
        "Edit" => Some("编辑"),
        "Remove" => Some("移除"),
        "Disconnect" => Some("断开连接"),
        "Mount" => Some("挂载"),
        "Unmount" => Some("卸载"),
        "Safely Remove" => Some("安全移除"),
        "Eject" => Some("弹出"),
        "Start indexing" => Some("开始建立索引"),
        "Build a search index?" => Some("要建立搜索索引吗？"),
        "Common locations" => Some("常用位置"),
        "Desktop, documents, downloads, media, and user config." => {
            Some("桌面、文档、下载、媒体目录和用户配置目录。")
        }
        "Custom selection" => Some("自定义选择"),
        "Choose folders or files from Home." => Some("从主目录中选择文件夹或文件。"),
        "Filenames" => Some("文件名"),
        "Filename and path catalog." => Some("建立文件名和路径目录。"),
        "Filenames + images" => Some("文件名 + 图像"),
        "Filename/path catalog plus image metadata." => Some("建立文件名/路径目录，并附带图像元数据。"),
        "No common locations found" => Some("未找到常用位置"),
        "No indexable locations found" => Some("未找到可索引的位置"),
        "Loading..." => Some("加载中..."),
        "Show hidden content" => Some("显示隐藏内容"),
        "Search Index" => Some("搜索索引"),
        "Refresh" => Some("刷新"),
        "Index directory" => Some("索引目录"),
        "Search mode" => Some("搜索模式"),
        "Simple search" => Some("简单搜索"),
        "Live filename and path search. No index is built or maintained." => {
            Some("实时搜索文件名和路径。不构建也不维护索引。")
        }
        "Indexed search" => Some("索引搜索"),
        "Uses configured indexed paths and optional media metadata indexing." => {
            Some("使用已配置的索引路径，并可选建立媒体元数据索引。")
        }
        "Profile" => Some("配置档"),
        "Service" => Some("服务"),
        "Profile id" => Some("配置档 ID"),
        "Roots" => Some("根路径"),
        "Restart service" => Some("重启服务"),
        "Delete profile" => Some("删除配置档"),
        "Filename and path catalog" => Some("文件名和路径目录"),
        "Unreadable directories" => Some("不可读目录"),
        "Skip" => Some("跳过"),
        "Abort" => Some("中止"),
        "No media metadata" => Some("不索引媒体元数据"),
        "Filename search only for images, audio, and video." => {
            Some("仅按文件名搜索图像、音频和视频。")
        }
        "Image metadata" => Some("图像元数据"),
        "Image dimensions and EXIF, without audio or video probing." => {
            Some("索引图像尺寸和 EXIF，不探测音频或视频。")
        }
        "All media metadata" => Some("全部媒体元数据"),
        "Image metadata plus audio and video metadata." => {
            Some("包含图像元数据，以及音频和视频元数据。")
        }
        "Changing media indexing applies to future rebuilds." => {
            Some("媒体元数据索引设置会在后续重建时生效。")
        }
        "Checking..." => Some("检查中..."),
        "Connected" => Some("已连接"),
        "Unknown" => Some("未知"),
        "Skip and record" => Some("跳过并记录"),
        "Abort scan" => Some("中止扫描"),
        "Path rules" => Some("路径规则"),
        "No path rules configured." => Some("尚未配置路径规则。"),
        "Add" => Some("添加"),
        "Modify" => Some("修改"),
        "Index" => Some("索引"),
        "Exclude" => Some("排除"),
        "Indexed roots" => Some("已索引根路径"),
        "No searchable roots are available yet." => Some("当前还没有可搜索的根路径。"),
        "Index path" => Some("索引路径"),
        "Indexing is queued or running." => Some("索引任务已排队或正在运行。"),
        "Loading index status..." => Some("正在加载索引状态..."),
        "Needs rebuild" => Some("需要重建"),
        "Present" => Some("已存在"),
        "Missing" => Some("缺失"),
        "State" => Some("状态"),
        "Records" => Some("记录数"),
        "Size" => Some("大小"),
        "Last update" => Some("最后更新"),
        "Failures" => Some("失败记录"),
        "Reason" => Some("原因"),
        "Update" => Some("更新"),
        "Rebuild" => Some("重建"),
        "Delete index" => Some("删除索引"),
        "Clear failures" => Some("清除失败记录"),
        "Out of range" => Some("超出范围"),
        "Preview" => Some("预览"),
        "Select a file and press Space to load preview" => Some("选择一个文件并按空格键加载预览"),
        "Loading preview..." => Some("正在加载预览..."),
        "Preparing download..." => Some("准备下载中..."),
        "Empty directory" => Some("空目录"),
        "Empty archive" => Some("空归档"),
        "Select an item to preview" => Some("请选择一个项目进行预览"),
        "Properties" => Some("属性"),
        "No properties are available." => Some("没有可用属性。"),
        "Loading properties..." => Some("正在加载属性..."),
        "Could not load properties" => Some("无法加载属性"),
        "File Information" => Some("文件信息"),
        "Permissions" => Some("权限"),
        "Type" => Some("类型"),
        "Location" => Some("位置"),
        "Created" => Some("创建时间"),
        "Modified" => Some("修改时间"),
        "Accessed" => Some("访问时间"),
        "Size on disk" => Some("磁盘占用"),
        "Calculating..." => Some("计算中..."),
        "Sharing & Permissions" => Some("共享与权限"),
        "Choose who can read, write, or execute this item. Changes save immediately." => {
            Some("选择谁可以读取、写入或执行此项目。更改会立即保存。")
        }
        "Current mode" => Some("当前模式"),
        "Privilege" => Some("权限级别"),
        "Owner" => Some("所有者"),
        "Group" => Some("群组"),
        "Everyone" => Some("所有人"),
        "Primary user" => Some("主要用户"),
        "Assigned group" => Some("所属群组"),
        "Other users" => Some("其他用户"),
        "Read, Write & Execute" => Some("读取、写入和执行"),
        "Read & Write" => Some("读取和写入"),
        "Read & Execute" => Some("读取和执行"),
        "Read only" => Some("只读"),
        "Write & Execute" => Some("写入和执行"),
        "Write only" => Some("只写"),
        "Execute only" => Some("仅执行"),
        "No access" => Some("无权限"),
        "Read" => Some("读取"),
        "Write" => Some("写入"),
        "Execute" => Some("执行"),
        "Apply to Enclosed Items" => Some("应用到包含的项目"),
        "Permissions are read-only" => Some("权限为只读"),
        "Permission editing is unavailable for this item." => Some("此项目不支持编辑权限。"),
        "Saving permissions..." => Some("正在保存权限..."),
        "Applying permissions to enclosed items..." => Some("正在将权限应用到包含的项目..."),
        "Unavailable" => Some("不可用"),
        "Folder" => Some("文件夹"),
        "Sort" => Some("排序"),
        "Extension" => Some("扩展名"),
        "Case" => Some("大小写"),
        "Sequence" => Some("序号"),
        "Replace" => Some("替换"),
        "Insert" => Some("插入"),
        "Slice" => Some("切片"),
        "Random" => Some("随机"),
        "List" => Some("列表"),
        "Custom" => Some("自定义"),
        "Regex" => Some("正则"),
        "Batch" => Some("批处理"),
        "Selection order" => Some("选择顺序"),
        "Natural" => Some("自然排序"),
        "Name A-Z" => Some("名称 A-Z"),
        "Name Z-A" => Some("名称 Z-A"),
        "Modified old-new" => Some("修改时间 旧到新"),
        "Modified new-old" => Some("修改时间 新到旧"),
        "Extension A-Z" => Some("扩展名 A-Z"),
        "Extension Z-A" => Some("扩展名 Z-A"),
        "Reverse" => Some("反转"),
        "Preserve" => Some("保留"),
        "lowercase" => Some("小写"),
        "UPPERCASE" => Some("大写"),
        "First" => Some("首个"),
        "Last" => Some("最后一个"),
        "Range" => Some("范围"),
        "Before" => Some("之前"),
        "After" => Some("之后"),
        "Position" => Some("位置"),
        "After text" => Some("在文本之后"),
        "Off" => Some("关闭"),
        "Replace stem" => Some("替换文件名主体"),
        "Prefix" => Some("前缀"),
        "Suffix" => Some("后缀"),
        "Text / Range" => Some("文本 / 范围"),
        "Character classes" => Some("字符类别"),
        "Lowercase" => Some("小写字母"),
        "Uppercase" => Some("大写字母"),
        "Digits" => Some("数字"),
        "Symbols" => Some("符号"),
        "Brackets" => Some("括号"),
        "Whitespace" => Some("空白字符"),
        "Hanzi" => Some("汉字"),
        "Keep case" => Some("保留大小写"),
        "Title Case" => Some("标题式大小写"),
        "Invert Case" => Some("反转大小写"),
        "Ready" => Some("就绪"),
        "Unchanged" => Some("未更改"),
        "Empty name" => Some("名称为空"),
        "Duplicate target" => Some("目标重复"),
        "Already exists" => Some("已存在"),
        "Rule error" => Some("规则错误"),
        "Original stem" => Some("原始名称主体"),
        "Ignore case" => Some("忽略大小写"),
        "Ignore extension" => Some("忽略扩展名"),
        "Start" => Some("起始"),
        "Step" => Some("步长"),
        "Padding" => Some("补位"),
        "Find" => Some("查找"),
        "With" => Some("替换为"),
        "Range start" => Some("范围起点"),
        "Range length" => Some("范围长度"),
        "Text" => Some("文本"),
        "Length" => Some("长度"),
        "Alphabet" => Some("字符集"),
        "Names" => Some("名称列表"),
        "Template" => Some("模板"),
        "Pattern" => Some("模式"),
        "Replacement" => Some("替换文本"),
        "Commands" => Some("命令"),
        "Apply" => Some("应用"),
        "Click a shortcut, then press the replacement keys." => {
            Some("点击一个快捷键项，然后按下新的按键组合。")
        }
        "Press keys..." => Some("请按键..."),
        "Reset" => Some("重置"),
        "Unsupported shortcut. Use a letter, number, function key, arrow, or named edit key." => {
            Some("不支持的快捷键。请使用字母、数字、功能键、方向键或命名编辑键。")
        }
        "Shortcut" => Some("快捷键"),
        "Places" => Some("位置"),
        "Favorites" => Some("收藏夹"),
        "Devices" => Some("设备"),
        "Loading devices..." => Some("正在加载设备..."),
        "Devices unavailable" => Some("设备不可用"),
        "No saved connections" => Some("没有已保存的连接"),
        "Network status unavailable" => Some("网络状态不可用"),
        "Network" => Some("网络"),
        "Working..." => Some("处理中..."),
        "Not connected" => Some("未连接"),
        "Connecting..." => Some("连接中..."),
        "Connection error" => Some("连接错误"),
        "Mounted" => Some("已挂载"),
        "Not mounted" => Some("未挂载"),
        "Choose search mode" => Some("选择搜索模式"),
        "Simple Search" => Some("简单搜索"),
        "Find file names live without building an index." => Some("无需建立索引，实时查找文件名。"),
        "Indexed Search" => Some("索引搜索"),
        "Build indexed paths for faster filename, path, and media search. File contents use live rg search." => {
            Some("建立索引路径，以更快搜索文件名、路径和媒体；文件内容始终使用实时 rg 搜索。")
        }
        "Next" => Some("下一步"),
        "Format" => Some("格式"),
        "Compression" => Some("压缩"),
        "tar.gz archives do not support passwords." => Some("tar.gz 归档不支持密码。"),
        "Create" => Some("创建"),
        "No selected items" => Some("没有已选择的项目"),
        "Checking" => Some("检查"),
        "Extract" => Some("解压"),
        "Checking password..." => Some("正在检查密码..."),
        "Enter the archive password to continue." => Some("请输入归档密码以继续。"),
        "Checking archive..." => Some("正在检查归档..."),
        "Thumbnails" => Some("缩略图"),
        "File preview" => Some("文件预览"),
        "Network List Thumbnails" => Some("网络列表缩略图"),
        "When enabled, images and videos on network locations may use more data." => {
            Some("启用后，网络位置中的图片和视频可能会使用更多流量。")
        }
        "Max File Preview" => Some("最大文件预览"),
        "MiB" => Some("MiB"),
        "Strong Verification" => Some("强校验"),
        "Compare copied file content hashes after standard metadata checks." => {
            Some("在标准元数据检查后，对复制文件的内容哈希再做比较。")
        }
        "Focus Path" => Some("聚焦路径栏"),
        "Back" => Some("后退"),
        "Forward" => Some("前进"),
        "Up Folder" => Some("上级文件夹"),
        "Select Up" => Some("选择上一个"),
        "Select Down" => Some("选择下一个"),
        "Select Parent Column" => Some("选择父列"),
        "Select Child Column" => Some("选择子列"),
        "Search Alternate" => Some("备用搜索"),
        "Dismiss" => Some("关闭当前层"),
        "Select All" => Some("全选"),
        "Copy Named Key" => Some("复制命名键"),
        "Paste Named Key" => Some("粘贴命名键"),
        "Cut" => Some("剪切"),
        "Cut Named Key" => Some("剪切命名键"),
        "Undo File Operation" => Some("撤销文件操作"),
        "Redo File Operation" => Some("重做文件操作"),
        "Automatic" => Some("自动"),
        "Ghostty" => Some("Ghostty"),
        "GNOME Console" => Some("GNOME Console"),
        "GNOME Terminal" => Some("GNOME 终端"),
        "Konsole" => Some("Konsole"),
        "Xfce Terminal" => Some("Xfce 终端"),
        "Tilix" => Some("Tilix"),
        "Alacritty" => Some("Alacritty"),
        "Foot" => Some("Foot"),
        "Kitty" => Some("Kitty"),
        "WezTerm" => Some("WezTerm"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_chinese_from_lang_prefix() {
        let language = detect_system_language_with(|key| match key {
            "LANG" => Some("zh_CN.UTF-8".to_owned()),
            _ => None,
        });

        assert_eq!(language, UiLanguage::Chinese);
    }

    #[test]
    fn detects_chinese_from_language_fallback() {
        let language = detect_system_language_with(|key| match key {
            "LANGUAGE" => Some("en_US:zh_CN".to_owned()),
            _ => None,
        });

        assert_eq!(language, UiLanguage::Chinese);
    }

    #[test]
    fn defaults_to_english_for_non_chinese_locale() {
        let language = detect_system_language_with(|key| match key {
            "LANG" => Some("en_US.UTF-8".to_owned()),
            _ => None,
        });

        assert_eq!(language, UiLanguage::English);
    }

    #[test]
    fn translates_known_static_text() {
        assert_eq!(translate(UiLanguage::Chinese, "Settings"), "设置");
    }

    #[test]
    fn translates_known_dynamic_text() {
        assert_eq!(
            translate(
                UiLanguage::Chinese,
                "Only showing 50 lines. Full line count: 200."
            ),
            "当前仅显示 50 行，完整行数为 200。"
        );
    }
}
