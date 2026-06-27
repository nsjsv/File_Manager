use std::io::Write;

use smithay_client_toolkit::{
    compositor::CompositorHandler,
    data_device_manager::{
        data_device::DataDeviceHandler, data_offer::DataOfferHandler,
        data_source::DataSourceHandler, WritePipe,
    },
    delegate_compositor, delegate_data_device, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT},
        Capability, SeatHandler,
    },
    shell::{
        xdg::window::{Window, WindowConfigure, WindowHandler},
        WaylandSurface,
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::DndAction,
        wl_data_source::WlDataSource, wl_output, wl_pointer::WlPointer, wl_seat::WlSeat,
        wl_surface,
    },
    Connection, QueueHandle,
};

use super::{log_mime_types, pick_mime, WaylandDndDemo, WINDOW_HEIGHT, WINDOW_WIDTH};

impl CompositorHandler for WaylandDndDemo {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.draw(conn, qh);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for WaylandDndDemo {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl WindowHandler for WaylandDndDemo {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        self.width = configure
            .new_size
            .0
            .map(|width| width.get())
            .unwrap_or(WINDOW_WIDTH);
        self.height = configure
            .new_size
            .1
            .map(|height| height.get())
            .unwrap_or(WINDOW_HEIGHT);
        self.buffer = None;
        eprintln!(
            "[dnd-demo] window: configured width={} height={}",
            self.width, self.height
        );
        if self.first_configure {
            self.first_configure = false;
            self.draw(conn, qh);
        }
    }
}

impl SeatHandler for WaylandDndDemo {
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
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => {
                    self.seat_objects[seat_index].pointer = Some(pointer);
                    eprintln!("[dnd-demo] seat: pointer capability ready index={seat_index}");
                }
                Err(error) => {
                    eprintln!("[dnd-demo] seat: get pointer failed error={error:?}");
                }
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
                eprintln!("[dnd-demo] seat: pointer capability removed");
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
            eprintln!("[dnd-demo] seat: removed");
            false
        });
    }
}

impl PointerHandler for WaylandDndDemo {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.window.wl_surface() {
                continue;
            }

            match event.kind {
                PointerEventKind::Enter { serial } => {
                    eprintln!(
                        "[dnd-demo] pointer: enter serial={serial} position={:?}",
                        event.position
                    );
                }
                PointerEventKind::Leave { serial } => {
                    eprintln!("[dnd-demo] pointer: leave serial={serial}");
                }
                PointerEventKind::Press { button, serial, .. } if button == BTN_LEFT => {
                    eprintln!(
                        "[dnd-demo] pointer: left press serial={serial} position={:?}",
                        event.position
                    );
                    self.start_drag_from_pointer(qh, pointer, &event.surface, serial);
                }
                PointerEventKind::Press { button, serial, .. } => {
                    eprintln!(
                        "[dnd-demo] pointer: press button={button:#x} serial={serial} position={:?}",
                        event.position
                    );
                }
                PointerEventKind::Release { button, serial, .. } => {
                    eprintln!(
                        "[dnd-demo] pointer: release button={button:#x} serial={serial} position={:?}",
                        event.position
                    );
                }
                PointerEventKind::Motion { .. } | PointerEventKind::Axis { .. } => {}
            }
        }
    }
}

impl ShmHandler for WaylandDndDemo {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

impl DataDeviceHandler for WaylandDndDemo {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
        x: f64,
        y: f64,
        _surface: &wl_surface::WlSurface,
    ) {
        eprintln!("[dnd-demo] drop-in: enter x={x:.1} y={y:.1}");
        let Some(data_device) = self.data_device_for(data_device) else {
            eprintln!("[dnd-demo] drop-in: enter skipped reason=unknown-data-device");
            return;
        };
        let Some(offer) = data_device.data().drag_offer() else {
            eprintln!("[dnd-demo] drop-in: internal drag enter");
            return;
        };

        offer.with_mime_types(|mime_types| {
            log_mime_types("drop-in", mime_types);
        });

        if let Some(mime_type) = offer.with_mime_types(pick_mime) {
            eprintln!("[dnd-demo] drop-in: accepting mime={mime_type}");
            offer.accept_mime_type(offer.serial, Some(mime_type));
            offer.set_actions(DndAction::Copy, DndAction::Copy);
        } else {
            eprintln!("[dnd-demo] drop-in: rejecting offer reason=unsupported-mime");
            offer.accept_mime_type(offer.serial, None);
            offer.set_actions(DndAction::empty(), DndAction::empty());
        }
    }

    fn leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _data_device: &WlDataDevice) {
        eprintln!("[dnd-demo] drop-in: leave");
    }

    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
        x: f64,
        y: f64,
    ) {
        eprintln!("[dnd-demo] drop-in: motion x={x:.1} y={y:.1}");
    }

    fn selection(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
    ) {
        let Some(data_device) = self.data_device_for(data_device) else {
            return;
        };
        if let Some(offer) = data_device.data().selection_offer() {
            offer.with_mime_types(|mime_types| {
                log_mime_types("selection", mime_types);
            });
        }
    }

    fn drop_performed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
    ) {
        eprintln!("[dnd-demo] drop-in: drop performed");
        let Some(data_device) = self.data_device_for(data_device) else {
            eprintln!("[dnd-demo] drop-in: drop skipped reason=unknown-data-device");
            return;
        };
        let Some(offer) = data_device.data().drag_offer() else {
            eprintln!("[dnd-demo] drop-in: internal drop performed");
            return;
        };
        let Some(mime_type) = offer.with_mime_types(pick_mime) else {
            eprintln!("[dnd-demo] drop-in: drop skipped reason=unsupported-mime");
            offer.finish();
            offer.destroy();
            return;
        };

        offer.accept_mime_type(offer.serial, Some(mime_type.clone()));
        offer.set_actions(DndAction::Copy, DndAction::Copy);
        self.register_drop_read(offer, mime_type);
    }
}

impl DataOfferHandler for WaylandDndDemo {
    fn source_actions(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        offer: &mut smithay_client_toolkit::data_device_manager::data_offer::DragOffer,
        actions: DndAction,
    ) {
        eprintln!("[dnd-demo] drop-in: source actions={actions:?}");
        offer.set_actions(DndAction::Copy, DndAction::Copy);
    }

    fn selected_action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut smithay_client_toolkit::data_device_manager::data_offer::DragOffer,
        actions: DndAction,
    ) {
        eprintln!("[dnd-demo] drop-in: selected action={actions:?}");
    }
}

impl DataSourceHandler for WaylandDndDemo {
    fn accept_mime(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        source: &WlDataSource,
        mime: Option<String>,
    ) {
        let known_source = self
            .drag_sources
            .iter()
            .any(|drag_session| drag_session.source.inner() == source);
        eprintln!("[dnd-demo] drag-out: target accepted mime={mime:?} known={known_source}");
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
            .any(|drag_session| drag_session.source.inner() == source);
        if !known_source {
            eprintln!("[dnd-demo] drag-out: send skipped reason=unknown-source mime={mime}");
            return;
        }

        let Some(payload) = self.sample_payload.for_mime(&mime) else {
            eprintln!("[dnd-demo] drag-out: send skipped reason=unsupported-mime mime={mime}");
            return;
        };

        match write_pipe.write_all(payload.as_bytes()) {
            Ok(()) => {
                eprintln!(
                    "[dnd-demo] drag-out: sent mime={mime} bytes={}",
                    payload.len()
                );
            }
            Err(error) => {
                eprintln!("[dnd-demo] drag-out: send failed mime={mime} error={error}");
            }
        }
    }

    fn cancelled(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, source: &WlDataSource) {
        let removed = self.remove_drag_source(source);
        eprintln!("[dnd-demo] drag-out: cancelled removed={removed}");
    }

    fn dnd_dropped(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, source: &WlDataSource) {
        let known_source = self
            .drag_sources
            .iter()
            .any(|drag_session| drag_session.source.inner() == source);
        eprintln!("[dnd-demo] drag-out: drop performed known={known_source}");
    }

    fn dnd_finished(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, source: &WlDataSource) {
        let removed = self.remove_drag_source(source);
        eprintln!("[dnd-demo] drag-out: finished removed={removed}");
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
            eprintln!("[dnd-demo] drag-out: selected action={action:?}");
        } else {
            eprintln!("[dnd-demo] drag-out: action for unknown source={action:?}");
        }
    }
}

impl ProvidesRegistryState for WaylandDndDemo {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, smithay_client_toolkit::seat::SeatState];
}

delegate_compositor!(WaylandDndDemo);
delegate_output!(WaylandDndDemo);
delegate_shm!(WaylandDndDemo);
delegate_seat!(WaylandDndDemo);
delegate_pointer!(WaylandDndDemo);
delegate_xdg_shell!(WaylandDndDemo);
delegate_xdg_window!(WaylandDndDemo);
delegate_data_device!(WaylandDndDemo);
delegate_registry!(WaylandDndDemo);
