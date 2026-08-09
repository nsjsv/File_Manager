use iced::widget::{container, image, scrollable, Column, Space};
use iced::{Alignment, ContentFit, Element, Length};

use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::appearance::{
    app_content_style, auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction,
    document_page_style, preview_window_panel_style,
};
use crate::document_preview::{
    document_viewport_height, DocumentPageView, DocumentPreviewMessage, PagedDocumentPreview,
};
use crate::model::{Message, PreviewSize, ScrollbarRegion, ScrollbarVisibility};
use crate::typography::localized_text;

const DOCUMENT_PANEL_PADDING: f32 = 14.0;
const DOCUMENT_SCROLLBAR_WIDTH: f32 = 6.0;
const DOCUMENT_PAGE_GAP: f32 = 12.0;
const DOCUMENT_STATUS_TEXT_SIZE: u32 = 13;

pub(super) fn document_preview_panel(
    document: &PagedDocumentPreview,
    preview_size: PreviewSize,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'static, Message> {
    let wanted_pages = document.wanted_pages();
    let mut pages = Column::new().width(Length::Fill).align_x(Alignment::Center);
    let top_spacer = document.top_spacer_height();
    if top_spacer > 0.0 {
        pages = pages.push(Space::new().height(Length::Fixed(top_spacer)));
    }

    for (position, page_index) in wanted_pages.iter().copied().enumerate() {
        let Some(layout) = document.page_layout(page_index) else {
            continue;
        };
        let page: Element<'static, Message> = match document.page_view(page_index) {
            DocumentPageView::Ready(handle) => image(handle.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(ContentFit::Contain)
                .into(),
            DocumentPageView::Error(error) => localized_text(error.to_owned())
                .size(DOCUMENT_STATUS_TEXT_SIZE)
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .into(),
            DocumentPageView::Loading => localized_text("Rendering page...")
                .size(DOCUMENT_STATUS_TEXT_SIZE)
                .into(),
            DocumentPageView::Deferred => localized_text("Page deferred by preview memory limit")
                .size(DOCUMENT_STATUS_TEXT_SIZE)
                .into(),
        };
        let page = container(page)
            .width(Length::Fixed(document.page_width()))
            .height(Length::Fixed(layout.height))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .clip(true)
            .style(document_page_style);
        pages = pages.push(page);
        if position + 1 < wanted_pages.len() {
            pages = pages.push(Space::new().height(Length::Fixed(DOCUMENT_PAGE_GAP)));
        }
    }

    let bottom_spacer = document.bottom_spacer_height();
    if bottom_spacer > 0.0 {
        pages = pages.push(Space::new().height(Length::Fixed(bottom_spacer)));
    }

    let region = ScrollbarRegion::PreviewDocument;
    let key = document.viewport_key();
    let scroller = scrollable(smooth_scroll_content(pages, region.clone()))
        .id(smooth_scroll_id(&region))
        .direction(auto_hide_vertical_scrollbar_direction(
            scrollbar_visibility,
            DOCUMENT_SCROLLBAR_WIDTH,
        ))
        .style(auto_hide_scrollbar_style(scrollbar_visibility))
        .width(Length::Fill)
        .height(Length::Fixed(document_viewport_height(preview_size.height)))
        .on_scroll(move |viewport| {
            Message::DocumentPreview(DocumentPreviewMessage::Scrolled {
                key: key.clone(),
                offset_y: viewport.absolute_offset().y,
                viewport_height: viewport.bounds().height,
                content_height: viewport.content_bounds().height,
            })
        });
    let surface = container(scroller)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(DOCUMENT_PANEL_PADDING)
        .style(preview_window_panel_style);

    container(surface)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(app_content_style)
        .into()
}
