//! 主窗口底部内嵌终端面板:底部窄条 + 可展开的真终端(PTY + 终端模拟)。
//!
//! 会话生命周期与面板可见性解耦:收起不杀 shell,`exit` 后自动收起。

pub(crate) mod emulator;
pub(crate) mod input;
pub(crate) mod session;
pub(crate) mod view;

use std::path::PathBuf;

use iced::advanced::subscription::{self, EventStream, Hasher, Recipe};
use iced::futures::stream::BoxStream;
use iced::{Point, Task};

use crate::app::FileBrowser;
use crate::model::Message;

use session::{SessionId, TerminalSession};

/// 展开时的默认面板高度(px)。
pub(crate) const DEFAULT_PANEL_HEIGHT: f32 = 320.0;
/// 拖拽调高的下限。
pub(crate) const MIN_PANEL_HEIGHT: f32 = 120.0;
/// 拖拽调高的上限(窗口高度比例)。
pub(crate) const MAX_PANEL_HEIGHT_RATIO: f32 = 0.8;
/// 面板内容左右留白;列数计算用。
pub(crate) const PANEL_HORIZONTAL_PADDING: f32 = 12.0;
/// 面板高度动画的 tick 步数(60Hz 下约 8 帧)。
const HEIGHT_ANIMATION_STEPS: f32 = 8.0;
/// 面板未展开时高度。
const COLLAPSED_HEIGHT: f32 = 0.0;

#[derive(Debug, Clone)]
pub(crate) enum TerminalPanelMessage {
    ToggleRequested,
    PanelResizeDragStarted,
    OutputReceived {
        session: SessionId,
        bytes: Vec<u8>,
    },
    ProcessExited {
        session: SessionId,
    },
    GridPressed {
        position: Point,
    },
    GridDragged {
        position: Point,
    },
    GridWheelScrolled {
        lines: i32,
    },
    CopySelectionRequested,
    PasteReceived(Option<String>),
}

/// 高度动画:from → to 的缓动插值。
#[derive(Debug, Clone, Copy)]
struct HeightAnimation {
    from: f32,
    to: f32,
    step: f32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PanelResizeDrag {
    cursor_start_y: f32,
    height_start: f32,
}

pub(crate) struct TerminalPanelState {
    /// 用户意图:展开(true)/收起(false);height 动画跟随此值。
    expanded: bool,
    /// 当前渲染高度;0 = 完全收起。
    height: f32,
    /// 展开时的目标高度(拖拽记忆值)。
    expanded_height: f32,
    height_animation: Option<HeightAnimation>,
    resize_drag: Option<PanelResizeDrag>,
    pub(crate) session: Option<TerminalSession>,
    next_session_id: SessionId,
    /// 终端网格持有键盘焦点(点击面板获得;点击地址栏等输入会话自动让路)。
    pub(crate) focused: bool,
    /// 鼠标左键拖动选区进行中。
    selection_drag_active: bool,
    /// 已同步给 shell 的目录;None 表示待同步(新会话或刚展开)。
    followed_directory: Option<PathBuf>,
}

impl TerminalPanelState {
    pub(crate) fn new() -> Self {
        Self {
            expanded: false,
            height: COLLAPSED_HEIGHT,
            expanded_height: DEFAULT_PANEL_HEIGHT,
            height_animation: None,
            resize_drag: None,
            session: None,
            next_session_id: 1,
            focused: false,
            selection_drag_active: false,
            followed_directory: None,
        }
    }

    pub(crate) fn height(&self) -> f32 {
        self.height
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.height_animation.is_some() || self.resize_drag.is_some()
    }

    pub(crate) fn is_focused(&self) -> bool {
        self.expanded && self.focused && self.session.is_some()
    }

    fn start_height_animation(&mut self, target: f32) {
        self.height_animation = Some(HeightAnimation {
            from: self.height,
            to: target,
            step: 0.0,
        });
    }

    fn clamp_height_for_window(&self, height: f32, window_height: f32) -> f32 {
        height
            .max(MIN_PANEL_HEIGHT)
            .min((window_height * MAX_PANEL_HEIGHT_RATIO).max(MIN_PANEL_HEIGHT))
    }
}

impl FileBrowser {
    /// 快捷键语义:开着就彻底关闭(杀 shell),关着就新开。
    pub(crate) fn toggle_terminal_via_shortcut(&mut self) -> Task<Message> {
        if self.terminal_panel.expanded {
            self.close_terminal_panel()
        } else {
            self.toggle_terminal_panel()
        }
    }

    /// 彻底关闭终端:终止 shell 会话并收起面板。
    pub(crate) fn close_terminal_panel(&mut self) -> Task<Message> {
        self.terminal_panel.expanded = false;
        self.terminal_panel.focused = false;
        self.terminal_panel.followed_directory = None;
        if let Some(mut session) = self.terminal_panel.session.take() {
            session.terminate();
        }
        self.terminal_panel.start_height_animation(COLLAPSED_HEIGHT);
        Task::none()
    }

    pub(crate) fn toggle_terminal_panel(&mut self) -> Task<Message> {
        if self.terminal_panel.expanded {
            self.terminal_panel.expanded = false;
            self.terminal_panel.focused = false;
            self.terminal_panel
                .start_height_animation(COLLAPSED_HEIGHT);
        } else {
            self.terminal_panel.expanded = true;
            self.terminal_panel.focused = true;
            let target = self.terminal_panel.clamp_height_for_window(
                self.terminal_panel.expanded_height,
                self.main_window_height,
            );
            self.terminal_panel.expanded_height = target;
            if self.terminal_panel.session.is_none() {
                self.spawn_terminal_session_for_active_directory(target);
            }
            self.terminal_panel.start_height_animation(target);
        }
        Task::none()
    }

    /// 在活动窗格的目录里拉起新 shell。失败时面板保持收起。
    fn spawn_terminal_session_for_active_directory(&mut self, height: f32) {
        let cwd = self.terminal_panel_spawn_directory();
        let columns = self.terminal_panel_grid_columns();
        let rows = terminal_panel_grid_rows(height);
        let shell = self.terminal_panel_shell();
        let session_id = self.terminal_panel.next_session_id;
        self.terminal_panel.next_session_id += 1;

        match session::spawn_terminal_session(
            session_id,
            &shell,
            &cwd,
            emulator_dimensions(columns, rows),
        ) {
            Ok(spawned) => {
                // 新 shell 已在目标目录,避免紧接着再注入一次 cd。
                self.terminal_panel.followed_directory = Some(cwd);
                self.terminal_panel.session = Some(spawned);
            }
            Err(error) => {
                self.terminal_panel.expanded = false;
                tracing::error!("内嵌终端启动失败: {error}");
            }
        }
    }

    /// 终端跟随的目标目录:多栏视图取鼠标悬停的栏,其余视图取活动窗格目录。
    /// 多栏视图鼠标不在任何栏上时返回 None(保持现状,避免来回跳)。
    fn terminal_panel_follow_target(&self) -> Option<PathBuf> {
        let active_pane_id = self.pane_layout.active();
        let pane = self
            .panes
            .iter()
            .find(|pane| pane.id == active_pane_id)?;
        if pane.view_mode != crate::model::BrowserViewMode::Columns {
            return Some(pane.current_dir.clone());
        }
        self.pointer_hovered_column_directory()
    }

    /// 活动窗格目录变化时向 shell 注入 cd;前进/后退/侧边栏跳转统一跟随。
    /// 仅在面板展开时注入,避免惊动收起状态下后台会话里的前台程序。
    pub(crate) fn follow_terminal_panel_directory(&mut self) {
        let Some(directory) = self.terminal_panel_follow_target() else {
            return;
        };
        if self.terminal_panel.followed_directory.as_ref() == Some(&directory) {
            return;
        }
        if !self.terminal_panel.expanded {
            // 展开前不注入,但清掉旧记录,让下次展开时立即跟随。
            self.terminal_panel.followed_directory = None;
            return;
        }
        let Some(session) = self.terminal_panel.session.as_mut() else {
            return;
        };
        self.terminal_panel.followed_directory = Some(directory.clone());
        let quoted = directory.to_string_lossy().replace('\'', "'\\''");
        session.write_input(format!("cd -- '{quoted}'\r").as_bytes());
    }

    /// 点击终端抽屉以外的区域时把键盘焦点还给文件区;
    /// 否则面板一旦聚焦就永久吞掉文件列表快捷键。
    pub(crate) fn release_terminal_panel_focus_if_outside(&mut self) {
        if !self.terminal_panel.focused {
            return;
        }
        let drawer_top = if self.terminal_panel.expanded {
            self.main_window_height - self.terminal_panel.height()
        } else {
            self.main_window_height - view::BOTTOM_BAR_HEIGHT
        };
        if self.cursor_position.y < drawer_top {
            self.terminal_panel.focused = false;
            if let Some(session) = self.terminal_panel.session.as_mut() {
                session.emulator.clear_selection();
            }
        }
    }

    /// 用户在设置里选的 shell;未选(空)时跟随系统登录 shell。
    fn terminal_panel_shell(&self) -> String {
        if self.user_config().terminal_shell.is_empty() {
            session::shell_for_user()
        } else {
            self.user_config().terminal_shell.clone()
        }
    }

    fn terminal_panel_spawn_directory(&self) -> PathBuf {
        let active_pane_id = self.pane_layout.active();
        self.panes
            .iter()
            .find(|pane| pane.id == active_pane_id)
            .map(|pane| pane.current_dir.clone())
            .unwrap_or_else(|| self.current_dir.clone())
    }

    /// 内容区宽度能容纳的终端列数(抽屉在侧边栏占位右侧)。
    pub(crate) fn terminal_panel_grid_columns(&self) -> usize {
        let width =
            (self.main_window_width - self.sidebar_width - PANEL_HORIZONTAL_PADDING * 2.0).max(1.0);
        ((width / view::CELL_WIDTH).floor() as usize).max(2)
    }

    /// 面板高度稳定后把新尺寸同步给 PTY;动画期间跳过,结束时再收敛。
    pub(crate) fn sync_terminal_panel_size(&mut self) {
        if !self.terminal_panel.expanded {
            return;
        }
        let rows = terminal_panel_grid_rows(self.terminal_panel.height.max(MIN_PANEL_HEIGHT));
        let columns = self.terminal_panel_grid_columns();
        let Some(session) = self.terminal_panel.session.as_mut() else {
            return;
        };
        session.resize(emulator_dimensions(columns, rows));
    }

    /// 高度动画推进;由 [`FileBrowser::advance_window_animation_frame`] 驱动。
    pub(crate) fn advance_terminal_panel_height_animation(&mut self) {
        let Some(animation) = self.terminal_panel.height_animation else {
            return;
        };
        let step = (animation.step + 1.0) / HEIGHT_ANIMATION_STEPS;
        let eased = ease_out_cubic(step.min(1.0));
        self.terminal_panel.height =
            animation.from + (animation.to - animation.from) * eased;
        if step >= 1.0 {
            self.terminal_panel.height = animation.to;
            self.terminal_panel.height_animation = None;
        } else {
            self.terminal_panel.height_animation = Some(HeightAnimation {
                from: animation.from,
                to: animation.to,
                step,
            });
        }
        self.sync_terminal_panel_size();
    }

    /// 拖拽面板上边缘调高;鼠标位置由全局 CursorMoved 提供。
    pub(crate) fn update_terminal_panel_resize_drag(&mut self, position: Point) {
        let Some(drag) = self.terminal_panel.resize_drag else {
            return;
        };
        let dragged =
            drag.height_start + (drag.cursor_start_y - position.y);
        let clamped = self
            .terminal_panel
            .clamp_height_for_window(dragged, self.main_window_height);
        self.terminal_panel.expanded_height = clamped;
        self.terminal_panel.height = clamped;
        self.terminal_panel.height_animation = None;
        self.sync_terminal_panel_size();
    }

    pub(crate) fn start_terminal_panel_resize_drag(&mut self) {
        if !self.terminal_panel.expanded {
            return;
        }
        self.terminal_panel.resize_drag = Some(PanelResizeDrag {
            cursor_start_y: self.cursor_position.y,
            height_start: self.terminal_panel.height,
        });
    }

    /// 全局左键释放:结束拖拽调高与选区拖动。
    pub(crate) fn finish_terminal_panel_pointer_interaction(&mut self) {
        self.terminal_panel.resize_drag = None;
        self.terminal_panel.selection_drag_active = false;
    }

    pub(crate) fn handle_terminal_panel_message(
        &mut self,
        message: TerminalPanelMessage,
    ) -> Task<Message> {
        match message {
            TerminalPanelMessage::ToggleRequested => return self.toggle_terminal_panel(),
            TerminalPanelMessage::PanelResizeDragStarted => {
                self.start_terminal_panel_resize_drag();
            }
            TerminalPanelMessage::OutputReceived { session, bytes } => {
                if let Some(active) = &mut self.terminal_panel.session {
                    if active.id == session {
                        active.emulator.feed(&bytes);
                    }
                }
            }
            TerminalPanelMessage::ProcessExited { session } => {
                if let Some(active) = &mut self.terminal_panel.session {
                    if active.id == session {
                        active.mark_exited();
                        self.terminal_panel.session = None;
                        self.terminal_panel.focused = false;
                        if self.terminal_panel.expanded {
                            self.terminal_panel.expanded = false;
                            self.terminal_panel
                                .start_height_animation(COLLAPSED_HEIGHT);
                        }
                    }
                }
            }
            TerminalPanelMessage::GridPressed { position } => {
                self.terminal_panel.focused = true;
                self.terminal_panel.selection_drag_active = true;
                if let Some(session) = &mut self.terminal_panel.session {
                    let (row, column) =
                        view::grid_cell_for_position(position);
                    session.emulator.clear_selection();
                    session.emulator.begin_selection(row, column);
                }
            }
            TerminalPanelMessage::GridDragged { position } => {
                if self.terminal_panel.selection_drag_active {
                    if let Some(session) = &mut self.terminal_panel.session {
                        let (row, column) =
                            view::grid_cell_for_position(position);
                        session.emulator.extend_selection(row, column);
                    }
                }
            }
            TerminalPanelMessage::GridWheelScrolled { lines } => {
                if let Some(session) = &mut self.terminal_panel.session {
                    session.emulator.scroll_display(lines);
                }
            }
            TerminalPanelMessage::CopySelectionRequested => {
                if let Some(session) = &self.terminal_panel.session {
                    if let Some(text) = session.emulator.selection_string() {
                        return iced::clipboard::write::<Message>(text);
                    }
                }
            }
            TerminalPanelMessage::PasteReceived(text) => {
                if let Some(text) = text {
                    if let Some(session) = &self.terminal_panel.session {
                        session.write_input(text.replace("\r\n", "\r").as_bytes());
                    }
                }
            }
        }
        Task::none()
    }

    /// 终端聚焦时的键盘独占路由;返回 `None` 表示不归终端管,继续原有快捷键路由。
    pub(crate) fn terminal_panel_keyboard_input(
        &mut self,
        key: &iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
    ) -> Option<Task<Message>> {
        if !self.terminal_panel.is_focused() {
            return None;
        }
        if self.keyboard_input_session_is_active() {
            return None;
        }

        // 宿主保留键:开关面板、复制、粘贴。
        if self.shortcut_config().matching_action(key, modifiers) == Some(crate::shortcuts::ShortcutAction::ToggleTerminal) {
            return Some(self.toggle_terminal_via_shortcut());
        }
        if modifiers.control() && modifiers.shift() {
            match key.as_ref() {
                iced::keyboard::Key::Character("c") => {
                    return Some(
                        self.handle_terminal_panel_message(TerminalPanelMessage::CopySelectionRequested),
                    );
                }
                iced::keyboard::Key::Character("v") => {
                    return Some(iced::clipboard::read().map(|text| {
                        Message::TerminalPanel(TerminalPanelMessage::PasteReceived(text))
                    }));
                }
                _ => {}
            }
        }

        let Some(session) = self.terminal_panel.session.as_mut() else {
            return None;
        };
        let input = input::input_for_key(key, modifiers);
        match input {
            input::TerminalInput::None => {
                // 无修饰翻页键属于本地回看。
                let rows = session.emulator.dimensions().screen_lines as i32;
                match key.as_ref() {
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::PageUp) => {
                        session.emulator.scroll_display(rows / 2);
                    }
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::PageDown) => {
                        session.emulator.scroll_display(-(rows / 2));
                    }
                    _ => {}
                }
            }
            other => {
                let bytes = other.into_bytes();
                if !bytes.is_empty() {
                    session.write_input(&bytes);
                }
            }
        }
        Some(Task::none())
    }

    /// 终端面板的输出订阅;无会话时为空。同一会话重订阅时克隆读端继续读;
    /// 运行时按 session_id 去重,重复构造的 recipe(连同其读端)会被丢弃。
    pub(crate) fn terminal_output_subscription(&self) -> iced::Subscription<Message> {
        let Some(active) = &self.terminal_panel.session else {
            return iced::Subscription::none();
        };
        let session_id = active.id;
        let Ok(reader) = active.clone_reader() else {
            tracing::error!("内嵌终端输出订阅失败: 读端不可用");
            return iced::Subscription::none();
        };
        subscription::from_recipe(TerminalOutputRecipe { session_id, reader })
    }
}

/// 输出订阅 recipe:稳定键为会话 id。
struct TerminalOutputRecipe {
    session_id: SessionId,
    reader: Box<dyn std::io::Read + Send>,
}

impl Recipe for TerminalOutputRecipe {
    type Output = Message;

    fn hash(&self, state: &mut Hasher) {
        use std::hash::Hasher as _;
        state.write_u64(self.session_id);
    }

    fn stream(self: Box<Self>, _input: EventStream) -> BoxStream<'static, Message> {
        use iced::futures::StreamExt;
        Box::pin(
            session::terminal_output_stream(self.session_id, self.reader)
                .map(Message::TerminalPanel),
        )
    }
}

fn terminal_panel_grid_rows(height: f32) -> usize {
    ((height / view::CELL_HEIGHT).floor() as usize).max(2)
}

fn emulator_dimensions(columns: usize, rows: usize) -> emulator::TerminalDimensions {
    emulator::TerminalDimensions {
        columns,
        screen_lines: rows,
    }
}

fn ease_out_cubic(t: f32) -> f32 {
    let clamped = t.clamp(0.0, 1.0);
    1.0 - (1.0 - clamped).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> TerminalPanelState {
        TerminalPanelState::new()
    }

    #[test]
    fn new_panel_starts_collapsed() {
        let panel = state();
        assert_eq!(panel.height(), 0.0);
        assert!(!panel.is_focused());
    }

    #[test]
    fn height_clamp_respects_window_ratio() {
        let panel = state();
        assert_eq!(panel.clamp_height_for_window(2000.0, 800.0), 640.0);
        assert_eq!(panel.clamp_height_for_window(10.0, 800.0), MIN_PANEL_HEIGHT);
        assert_eq!(panel.clamp_height_for_window(300.0, 800.0), 300.0);
    }

    #[test]
    fn rows_follow_cell_height() {
        assert_eq!(terminal_panel_grid_rows(view::CELL_HEIGHT * 5.0), 5);
    }
}
