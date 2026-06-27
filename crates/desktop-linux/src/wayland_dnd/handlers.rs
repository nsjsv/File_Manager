use std::io::Write;

use smithay_client_toolkit::data_device_manager::{
    data_device::DataDeviceHandler, data_offer::DataOfferHandler, data_source::DataSourceHandler,
    WritePipe,
};
use smithay_client_toolkit::delegate_data_device;
use smithay_client_toolkit::delegate_pointer;
use smithay_client_toolkit::delegate_registry;
use smithay_client_toolkit::delegate_seat;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::registry_handlers;
use smithay_client_toolkit::seat::{
    pointer::{PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT},
    Capability, SeatHandler,
};
use wayland_client::protocol::{
    wl_data_device::WlDataDevice, wl_data_device_manager::DndAction, wl_data_source::WlDataSource,
    wl_pointer::WlPointer, wl_seat::WlSeat, wl_surface,
};
use wayland_client::{Connection, QueueHandle};

use super::{pick_mime, WaylandFileDnd};
use crate::wayland_dnd::WaylandDndDropPosition;

impl SeatHandler for WaylandFileDnd {
    fn seat_state(&mut self) -> &mut smithay_client_toolkit::seat::SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        let seat_index = self.ensure_seat_object(qh, &seat);
        if capability == Capability::Pointer && self.seat_objects[seat_index].pointer.is_none() {
            if let Ok(pointer) = self.seat_state.get_pointer(qh, &seat) {
                self.seat_objects[seat_index].pointer = Some(pointer);
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        if capability != Capability::Pointer {
            return;
        }

        if let Some(seat_object) = self
            .seat_objects
            .iter_mut()
            .find(|seat_object| seat_object.seat == seat)
        {
            if let Some(pointer) = seat_object.pointer.take() {
                pointer.release();
            }
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, seat: WlSeat) {
        self.seat_objects.retain_mut(|seat_object| {
            if seat_object.seat != seat {
                return true;
            }
            if let Some(pointer) = seat_object.pointer.take() {
                pointer.release();
            }
            false
        });
    }
}

impl PointerHandler for WaylandFileDnd {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if event.surface != self.surface {
                continue;
            }

            if let PointerEventKind::Press { button, serial, .. } = event.kind {
                if button == BTN_LEFT {
                    self.remember_pointer_press(qh, pointer, &event.surface, serial);
                }
            }
            if let PointerEventKind::Release { button, serial, .. } = event.kind {
                if button == BTN_LEFT {
                    self.clear_pointer_press(serial);
                }
            }
        }
    }
}

impl DataDeviceHandler for WaylandFileDnd {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
        x: f64,
        y: f64,
        surface: &wl_surface::WlSurface,
    ) {
        let Some(data_device) = self.data_device_for(data_device) else {
            return;
        };
        let Some(offer) = data_device.data().drag_offer() else {
            return;
        };
        self.drop_is_over_surface = surface == &self.surface;
        self.drop_position = self
            .drop_is_over_surface
            .then_some(WaylandDndDropPosition { x, y });
        if !self.drop_is_over_surface {
            offer.accept_mime_type(offer.serial, None);
            offer.set_actions(DndAction::empty(), DndAction::empty());
            return;
        }

        if let Some(mime_type) = offer.with_mime_types(pick_mime) {
            offer.accept_mime_type(offer.serial, Some(mime_type));
            offer.set_actions(DndAction::Copy, DndAction::Copy);
        } else {
            offer.accept_mime_type(offer.serial, None);
            offer.set_actions(DndAction::empty(), DndAction::empty());
        }
    }

    fn leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _data_device: &WlDataDevice) {
        self.drop_is_over_surface = false;
        self.drop_position = None;
    }

    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
        x: f64,
        y: f64,
    ) {
        if self.drop_is_over_surface {
            self.drop_position = Some(WaylandDndDropPosition { x, y });
        }
    }

    fn selection(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
    ) {
    }

    fn drop_performed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
    ) {
        let Some(data_device) = self.data_device_for(data_device) else {
            return;
        };
        let Some(offer) = data_device.data().drag_offer() else {
            return;
        };
        if !self.drop_is_over_surface {
            offer.finish();
            offer.destroy();
            return;
        }
        let Some(mime_type) = offer.with_mime_types(pick_mime) else {
            offer.finish();
            offer.destroy();
            return;
        };

        offer.accept_mime_type(offer.serial, Some(mime_type.clone()));
        offer.set_actions(DndAction::Copy, DndAction::Copy);
        self.register_drop_read(offer, mime_type);
    }
}

impl DataOfferHandler for WaylandFileDnd {
    fn source_actions(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        offer: &mut smithay_client_toolkit::data_device_manager::data_offer::DragOffer,
        _actions: DndAction,
    ) {
        offer.set_actions(DndAction::Copy, DndAction::Copy);
    }

    fn selected_action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut smithay_client_toolkit::data_device_manager::data_offer::DragOffer,
        _actions: DndAction,
    ) {
    }
}

impl DataSourceHandler for WaylandFileDnd {
    fn accept_mime(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _mime: Option<String>,
    ) {
    }

    fn send_request(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        source: &WlDataSource,
        mime: String,
        mut write_pipe: WritePipe,
    ) {
        let known_source = self
            .drag_sources
            .iter()
            .find(|drag_session| drag_session.source.inner() == source);
        let Some(drag_session) = known_source else {
            return;
        };

        if let Some(payload) = drag_session.payload.for_mime(&mime) {
            tracing::debug!(mime, "Wayland drag target requested payload");
            let _ = write_pipe.write_all(payload.as_bytes());
        }
    }

    fn cancelled(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, source: &WlDataSource) {
        tracing::debug!("Wayland file drag source cancelled");
        self.remove_drag_source(source);
    }

    fn dnd_dropped(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _source: &WlDataSource) {
        tracing::debug!("Wayland file drag source dropped");
    }

    fn dnd_finished(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, source: &WlDataSource) {
        tracing::debug!("Wayland file drag source finished");
        self.remove_drag_source(source);
    }

    fn action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        source: &WlDataSource,
        action: DndAction,
    ) {
        if let Some(drag_session) = self
            .drag_sources
            .iter_mut()
            .find(|drag_session| drag_session.source.inner() == source)
        {
            drag_session.selected_action = action;
        }
    }
}

impl ProvidesRegistryState for WaylandFileDnd {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![smithay_client_toolkit::seat::SeatState];
}

delegate_seat!(WaylandFileDnd);
delegate_pointer!(WaylandFileDnd);
delegate_data_device!(WaylandFileDnd);
delegate_registry!(WaylandFileDnd);
