use iced::widget::{
    button, column, container, progress_bar, row, scrollable, text_input, Column, Space,
};
use iced::{Alignment, Element, Length};

use file_core::{ChecksumAlgorithm, ALL_CHECKSUM_ALGORITHMS};

use crate::app::checksum::{
    ChecksumComputation, ChecksumExpectedVerdict, ChecksumFileVerdict, ChecksumMessage,
    ChecksumState,
};
use crate::appearance::{context_menu_style, muted_text_color};
use crate::formatting::format_middle_ellipsized_text;
use crate::icons::IconSymbol;
use crate::model::Message;
use crate::typography::readable_text;

use super::option_controls::secondary_action_button;
use super::settings_group::settings_card;
use super::{themed_icon, IconTone, MENU_ICON_SIZE};

const CHECKSUM_PANEL_WIDTH: f32 = 560.0;
const SECTION_SPACING: f32 = 12.0;
/// SHA-512 摘要 128 位太长,面板内显示中间省略,完整值走复制按钮。
const DIGEST_MAX_CHARS: usize = 52;
const FILE_CHIP_MAX_CHARS: usize = 26;
const ACTIVE_PATH_MAX_CHARS: usize = 62;
const CHECKSUM_FILE_PATH_MAX_CHARS: usize = 44;
const FILE_LIST_MAX_HEIGHT: f32 = 110.0;
/// 单个文件 chip 的高度(文本 12 + 上下 padding 4×2 + 行距 2 + 边距)。
const FILE_CHIP_ROW_HEIGHT: f32 = 26.0;
const NOTICE_ICON_SIZE: f32 = 14.0;

pub(super) fn checksum_panel(state: &ChecksumState) -> Element<'_, Message> {
    let title = row![
        themed_icon(IconSymbol::Hash, IconTone::Normal, MENU_ICON_SIZE),
        readable_text("File Checksum").size(16).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut content = column![
        title,
        readable_text(format_middle_ellipsized_text(
            &state.active_file().to_string_lossy(),
            ACTIVE_PATH_MAX_CHARS,
        ))
        .size(11)
        .width(Length::Fill)
    ]
    .spacing(SECTION_SPACING)
    .width(Length::Fill);

    if state.files().len() > 1 {
        content = content.push(file_chip_list(state));
    }

    content = content.push(computation_section(state));
    content = content.push(verify_section(state));
    content = content.push(
        row![
            Space::new().width(Length::Fill),
            secondary_action_button("Close", Message::DismissFloating),
        ]
        .align_y(Alignment::Center),
    );

    container(content)
        .padding(16)
        .width(Length::Fixed(CHECKSUM_PANEL_WIDTH))
        .style(context_menu_style)
        .into()
}

/// 多选文件切换列表:当前文件带对勾标记,点击其它文件切换并重新计算。
fn file_chip_list(state: &ChecksumState) -> Element<'static, Message> {
    let mut chips = Column::new().spacing(2).width(Length::Fill);
    for (index, path) in state.files().iter().enumerate() {
        let chip_icon = if index == state.active_index() {
            IconSymbol::Check
        } else {
            IconSymbol::File
        };
        let label = path
            .file_name()
            .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy());
        chips = chips.push(
            button(
                row![
                    themed_icon(chip_icon, IconTone::Normal, 12.0),
                    readable_text(format_middle_ellipsized_text(&label, FILE_CHIP_MAX_CHARS,))
                        .size(12)
                        .width(Length::Fill),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .on_press(Message::Checksum(ChecksumMessage::FileSelected(index)))
            .padding([4, 6])
            .width(Length::Fill)
            .style(crate::appearance::context_menu_item_button_style()),
        );
    }

    // 高度随文件数伸缩,超出上限才滚动。
    let list_height = (state.files().len() as f32 * FILE_CHIP_ROW_HEIGHT).min(FILE_LIST_MAX_HEIGHT);
    scrollable(chips)
        .height(Length::Fixed(list_height))
        .width(Length::Fill)
        .into()
}

fn computation_section(state: &ChecksumState) -> Element<'static, Message> {
    let body: Element<'static, Message> = match state.computation() {
        ChecksumComputation::Computing {
            bytes_done,
            total_bytes,
        } => {
            let fraction = if *total_bytes > 0 {
                *bytes_done as f32 / *total_bytes as f32
            } else {
                0.0
            };
            let percent = (fraction * 100.0).round() as u32;
            row![
                container(progress_bar(0.0..=1.0, fraction)).width(Length::Fill),
                readable_text("Computing...").size(12),
                readable_text(format!("{percent}%")).size(12),
                secondary_action_button(
                    "Cancel",
                    Message::Checksum(ChecksumMessage::CancelPressed)
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
        }
        ChecksumComputation::Canceled => retry_notice(crate::localization::translate_current(
            "Checksum computation canceled",
        ))
        .into(),
        ChecksumComputation::Failed(error) => retry_notice(error.clone()).into(),
        ChecksumComputation::Completed(digests) => {
            let rows = ALL_CHECKSUM_ALGORITHMS
                .into_iter()
                .map(|algorithm| digest_row(state, digests.digest(algorithm), algorithm))
                .collect();
            settings_card(rows)
        }
    };

    section(body)
}

fn digest_row(
    state: &ChecksumState,
    digest: &str,
    algorithm: ChecksumAlgorithm,
) -> Element<'static, Message> {
    let copy_icon = if state.last_copied() == Some(algorithm) {
        IconSymbol::Check
    } else {
        IconSymbol::Copy
    };
    row![
        readable_text(algorithm.label())
            .size(12)
            .width(Length::Fixed(64.0)),
        readable_text(format_middle_ellipsized_text(digest, DIGEST_MAX_CHARS))
            .size(12)
            .width(Length::Fill),
        button(themed_icon(copy_icon, IconTone::Normal, 13.0))
            .on_press(Message::Checksum(ChecksumMessage::HashCopyRequested(
                algorithm
            )))
            .padding(4)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .style(crate::appearance::navigation_icon_button_style()),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn verify_section(state: &ChecksumState) -> Element<'static, Message> {
    let mut rows = vec![text_input(
        &crate::localization::translate_current("Paste expected checksum"),
        state.expected_text(),
    )
    .on_input(|text| Message::Checksum(ChecksumMessage::ExpectedValueChanged(text)))
    .padding([6, 8])
    .size(13)
    .width(Length::Fill)
    .into()];

    if let Some(verdict) = expected_value_verdict_line(state.expected_value_verdict()) {
        rows.push(verdict);
    }

    let mut load_row = row![secondary_action_button(
        "Load checksum file...",
        Message::Checksum(ChecksumMessage::ChecksumFileLoadPressed)
    )]
    .spacing(8)
    .align_y(Alignment::Center);
    if let Some(verification) = state.checksum_file() {
        load_row = load_row.push(
            readable_text(format_middle_ellipsized_text(
                &verification.path().to_string_lossy(),
                CHECKSUM_FILE_PATH_MAX_CHARS,
            ))
            .size(11),
        );
    }
    rows.push(load_row.width(Length::Fill).into());

    if let Some(verdict) = state.checksum_file_verdict() {
        rows.push(checksum_file_verdict_line(verdict));
    }

    section(
        Column::with_children(rows)
            .spacing(6)
            .width(Length::Fill)
            .into(),
    )
}

/// 期望值比对结论行;没有输入时不占位。
fn expected_value_verdict_line(
    verdict: ChecksumExpectedVerdict,
) -> Option<Element<'static, Message>> {
    let localize = crate::localization::translate_current;
    let line = match verdict {
        ChecksumExpectedVerdict::Empty => return None,
        ChecksumExpectedVerdict::Invalid => verdict_line(
            IconSymbol::TriangleAlert,
            IconTone::Normal,
            localize("Enter a valid hex checksum."),
        )
        .into(),
        ChecksumExpectedVerdict::Pending => {
            muted_info_line(localize("Waiting for the computation to finish...")).into()
        }
        ChecksumExpectedVerdict::NoMatch => verdict_line(
            IconSymbol::TriangleAlert,
            IconTone::Warning,
            localize("No checksum matches the expected value."),
        )
        .into(),
        ChecksumExpectedVerdict::Matched(algorithms) => verdict_line(
            IconSymbol::Check,
            IconTone::Normal,
            matched_text(&localize("Checksum matches"), &algorithms),
        )
        .into(),
    };
    Some(line)
}

fn checksum_file_verdict_line(verdict: ChecksumFileVerdict) -> Element<'static, Message> {
    let localize = crate::localization::translate_current;
    match verdict {
        ChecksumFileVerdict::Pending => {
            muted_info_line(localize("Waiting for the computation to finish...")).into()
        }
        ChecksumFileVerdict::BareMatched => verdict_line(
            IconSymbol::Check,
            IconTone::Normal,
            localize("Checksum file value matches."),
        )
        .into(),
        ChecksumFileVerdict::BareMismatch => verdict_line(
            IconSymbol::TriangleAlert,
            IconTone::Warning,
            localize("Checksum file value does not match."),
        )
        .into(),
        ChecksumFileVerdict::EntryMatched => verdict_line(
            IconSymbol::Check,
            IconTone::Normal,
            localize("File entry in the checksum file matches."),
        )
        .into(),
        ChecksumFileVerdict::EntryMismatch => verdict_line(
            IconSymbol::TriangleAlert,
            IconTone::Warning,
            localize("File entry in the checksum file does not match."),
        )
        .into(),
        ChecksumFileVerdict::EntryNotFound => {
            muted_info_line(localize("The checksum file has no entry for this file.")).into()
        }
    }
}

fn matched_text(prefix: &str, algorithms: &[ChecksumAlgorithm]) -> String {
    let labels = algorithms
        .iter()
        .map(|algorithm| algorithm.label())
        .collect::<Vec<_>>()
        .join(" / ");
    format!("{prefix} ({labels})")
}

/// 分区:muted 标题 + 卡片行组;与设置窗口的分组观感一致。
fn section(body: Element<'static, Message>) -> Element<'static, Message> {
    column![
        container(readable_text("Verify"))
            .width(Length::Fill)
            .style(muted_container_style),
        body,
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

/// 取消/失败后的提示行,附重新计算按钮。
fn retry_notice(message: String) -> iced::widget::Row<'static, Message> {
    row![
        themed_icon(
            IconSymbol::TriangleAlert,
            IconTone::Warning,
            NOTICE_ICON_SIZE
        ),
        readable_text(message).size(12).width(Length::Fill),
        secondary_action_button("Retry", Message::Checksum(ChecksumMessage::RetryPressed)),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fill)
}

fn verdict_line(
    icon: IconSymbol,
    tone: IconTone,
    message: String,
) -> iced::widget::Row<'static, Message> {
    row![
        themed_icon(icon, tone, NOTICE_ICON_SIZE),
        readable_text(message).size(12).width(Length::Fill),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
}

fn muted_info_line(message: String) -> iced::widget::Text<'static> {
    readable_text(message).size(12)
}

fn muted_container_style(theme: &iced::Theme) -> container::Style {
    container::Style {
        text_color: Some(muted_text_color(theme)),
        ..container::Style::default()
    }
}
