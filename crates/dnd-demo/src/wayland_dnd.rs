use std::{
    convert::TryInto,
    fs,
    io::{BufRead, BufReader},
    path::PathBuf,
    time::Duration,
};

use dnd_demo::payload::DragPayload;
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle, PostAction};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::CompositorState,
    data_device_manager::{
        data_device::DataDevice, data_offer::DragOffer, data_source::DragSource,
        DataDeviceManagerState,
    },
    output::OutputState,
    registry::RegistryState,
    seat::SeatState,
    shell::{
        xdg::{
            window::{Window, WindowDecorations},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{
        slot::{Buffer, SlotPool},
        Shm,
    },
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::DndAction, wl_pointer::WlPointer,
        wl_seat::WlSeat, wl_shm, wl_surface,
    },
    Connection, QueueHandle,
};

mod handlers;

const APP_ID: &str = "file-manager-dnd-demo";
const WINDOW_WIDTH: u32 = 560;
const WINDOW_HEIGHT: u32 = 560;
const SUPPORTED_MIME_TYPES: &[&str] = &[
    "x-special/gnome-copied-files",
    "text/uri-list",
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
];

pub fn run(
    sample_path: PathBuf,
    sample_payload: DragPayload,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<WaylandDndDemo> = EventLoop::try_new()?;
    let loop_handle = event_loop.handle();
    WaylandSource::new(conn.clone(), event_queue).insert(loop_handle.clone())?;

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor not available");
    let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg shell is not available");
    let shm_state = Shm::bind(&globals, &qh).expect("wl_shm is not available");
    let data_device_state = DataDeviceManagerState::bind(&globals, &qh)
        .expect("wl_data_device_manager is not available");

    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_title("Wayland DnD Demo");
    window.set_app_id(APP_ID);
    window.set_min_size(Some((WINDOW_WIDTH, WINDOW_HEIGHT)));
    window.commit();

    eprintln!("[dnd-demo] startup: native Wayland window ready; left press starts file drag-out");

    let pool = SlotPool::new((WINDOW_WIDTH * WINDOW_HEIGHT * 4) as usize, &shm_state)?;
    let mut demo = WaylandDndDemo {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm_state,
        data_device_state,
        window,
        pool,
        buffer: None,
        width: WINDOW_WIDTH,
        height: WINDOW_HEIGHT,
        first_configure: true,
        exit: false,
        seat_objects: Vec::new(),
        drag_sources: Vec::new(),
        drop_reads: Vec::new(),
        loop_handle: event_loop.handle(),
        sample_path,
        sample_payload,
    };

    while !demo.exit {
        event_loop.dispatch(Duration::from_millis(16), &mut demo)?;
    }

    eprintln!("[dnd-demo] shutdown");
    Ok(())
}

struct WaylandDndDemo {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm_state: Shm,
    data_device_state: DataDeviceManagerState,
    window: Window,
    pool: SlotPool,
    buffer: Option<Buffer>,
    width: u32,
    height: u32,
    first_configure: bool,
    exit: bool,
    seat_objects: Vec<SeatObject>,
    drag_sources: Vec<DragSession>,
    drop_reads: Vec<DropRead>,
    loop_handle: LoopHandle<'static, WaylandDndDemo>,
    sample_path: PathBuf,
    sample_payload: DragPayload,
}

struct SeatObject {
    seat: WlSeat,
    pointer: Option<WlPointer>,
    data_device: DataDevice,
}

struct DragSession {
    source: DragSource,
    selected_action: DndAction,
}

struct DropRead {
    offer: DragOffer,
    mime_type: String,
    data: Vec<u8>,
}

impl WaylandDndDemo {
    fn ensure_seat_object(&mut self, qh: &QueueHandle<Self>, seat: &WlSeat) -> usize {
        if let Some(index) = self
            .seat_objects
            .iter()
            .position(|seat_object| seat_object.seat == *seat)
        {
            return index;
        }

        let data_device = self.data_device_state.get_data_device(qh, seat);
        self.seat_objects.push(SeatObject {
            seat: seat.clone(),
            pointer: None,
            data_device,
        });
        let index = self.seat_objects.len() - 1;
        eprintln!("[dnd-demo] seat: created data device for seat index={index}");
        index
    }

    fn data_device_for(&self, data_device: &WlDataDevice) -> Option<&DataDevice> {
        self.seat_objects
            .iter()
            .find(|seat_object| seat_object.data_device.inner() == data_device)
            .map(|seat_object| &seat_object.data_device)
    }

    fn start_drag_from_pointer(
        &mut self,
        qh: &QueueHandle<Self>,
        pointer: &WlPointer,
        surface: &wl_surface::WlSurface,
        serial: u32,
    ) {
        let Some(seat_index) = self
            .seat_objects
            .iter()
            .position(|seat_object| seat_object.pointer.as_ref() == Some(pointer))
        else {
            eprintln!("[dnd-demo] drag-out: skipped reason=unknown-pointer serial={serial}");
            return;
        };

        let source = self.data_device_state.create_drag_and_drop_source(
            qh,
            SUPPORTED_MIME_TYPES.to_vec(),
            DndAction::Move,
        );
        source.start_drag(
            &self.seat_objects[seat_index].data_device,
            surface,
            None,
            serial,
        );
        self.drag_sources.push(DragSession {
            source,
            selected_action: DndAction::empty(),
        });
        eprintln!(
            "[dnd-demo] drag-out: started serial={serial} path={} mimes={}",
            self.sample_path.display(),
            SUPPORTED_MIME_TYPES.join(", ")
        );
    }

    fn register_drop_read(&mut self, offer: DragOffer, mime_type: String) {
        let read_pipe = match offer.receive(mime_type.clone()) {
            Ok(read_pipe) => read_pipe,
            Err(error) => {
                eprintln!("[dnd-demo] drop-in: receive failed mime={mime_type} error={error:?}");
                offer.finish();
                offer.destroy();
                return;
            }
        };

        self.drop_reads.push(DropRead {
            offer: offer.clone(),
            mime_type: mime_type.clone(),
            data: Vec::new(),
        });

        let offer_key = offer.clone();
        match self
            .loop_handle
            .insert_source(read_pipe, move |_, file, state| {
                state.read_drop_payload(&offer_key, file)
            }) {
            Ok(_) => {
                eprintln!("[dnd-demo] drop-in: reading mime={mime_type}");
            }
            Err(error) => {
                self.drop_reads.retain(|read| read.offer != offer);
                eprintln!("[dnd-demo] drop-in: register read failed error={error}");
                offer.finish();
                offer.destroy();
            }
        }
    }

    fn read_drop_payload(
        &mut self,
        offer_key: &DragOffer,
        file: &mut smithay_client_toolkit::reexports::calloop::generic::NoIoDrop<fs::File>,
    ) -> PostAction {
        let Some(position) = self
            .drop_reads
            .iter()
            .position(|read| &read.offer == offer_key)
        else {
            return PostAction::Continue;
        };
        let mut drop_read = self.drop_reads.remove(position);
        let mut consumed = 0;

        let read_outcome = {
            let file = unsafe { file.get_mut() };
            let mut reader = BufReader::new(file);
            match reader.fill_buf() {
                Ok(buffer) if buffer.is_empty() => DropReadOutcome::Finished,
                Ok(buffer) => {
                    drop_read.data.extend_from_slice(buffer);
                    consumed = buffer.len();
                    DropReadOutcome::Continue
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                    DropReadOutcome::Continue
                }
                Err(error) => DropReadOutcome::Failed(error),
            }
            .also_consume(&mut reader, consumed)
        };

        match read_outcome {
            DropReadOutcome::Continue => {
                self.drop_reads.push(drop_read);
                PostAction::Continue
            }
            DropReadOutcome::Finished => {
                self.finish_drop_read(drop_read);
                PostAction::Remove
            }
            DropReadOutcome::Failed(error) => {
                eprintln!("[dnd-demo] drop-in: read failed error={error}");
                drop_read.offer.finish();
                drop_read.offer.destroy();
                PostAction::Remove
            }
        }
    }

    fn finish_drop_read(&mut self, drop_read: DropRead) {
        eprintln!(
            "[dnd-demo] drop-in: received mime={} bytes={}",
            drop_read.mime_type,
            drop_read.data.len()
        );
        log_payload_preview(&drop_read.data);
        drop_read.offer.finish();
        drop_read.offer.destroy();
    }

    fn remove_drag_source(
        &mut self,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
    ) -> bool {
        let before = self.drag_sources.len();
        self.drag_sources
            .retain(|drag_session| drag_session.source.inner() != source);
        before != self.drag_sources.len()
    }

    fn draw(&mut self, _conn: &Connection, qh: &QueueHandle<Self>) {
        let width = self.width;
        let height = self.height;
        let stride = width as i32 * 4;

        let buffer = self.buffer.get_or_insert_with(|| {
            self.pool
                .create_buffer(
                    width as i32,
                    height as i32,
                    stride,
                    wl_shm::Format::Argb8888,
                )
                .expect("create buffer")
                .0
        });

        let canvas = match self.pool.canvas(buffer) {
            Some(canvas) => canvas,
            None => {
                let (next_buffer, canvas) = self
                    .pool
                    .create_buffer(
                        width as i32,
                        height as i32,
                        stride,
                        wl_shm::Format::Argb8888,
                    )
                    .expect("create buffer");
                *buffer = next_buffer;
                canvas
            }
        };

        paint_window(canvas, width, height);
        self.window
            .wl_surface()
            .damage_buffer(0, 0, width as i32, height as i32);
        self.window
            .wl_surface()
            .frame(qh, self.window.wl_surface().clone());
        buffer
            .attach_to(self.window.wl_surface())
            .expect("buffer attach");
        self.window.commit();
    }
}

enum DropReadOutcome {
    Continue,
    Finished,
    Failed(std::io::Error),
}

impl DropReadOutcome {
    fn also_consume<R: BufRead>(self, reader: &mut R, consumed: usize) -> Self {
        if consumed > 0 {
            reader.consume(consumed);
        }
        self
    }
}

fn paint_window(canvas: &mut [u8], width: u32, height: u32) {
    for (index, chunk) in canvas.chunks_exact_mut(4).enumerate() {
        let x = (index % width as usize) as u32;
        let y = (index / width as usize) as u32;
        let vertical = y.saturating_mul(255) / height.saturating_sub(1).max(1);
        let center_distance = x.abs_diff(width / 2) + y.abs_diff(height / 2);
        let vignette = center_distance.saturating_mul(20) / width.saturating_add(height).max(1);
        let border = x < 8 || y < 8 || x + 8 >= width || y + 8 >= height;
        let inner_border = x < 18 || y < 18 || x + 18 >= width || y + 18 >= height;

        let (r, g, b) = if border {
            (166, 222, 138)
        } else if inner_border {
            (40, 132, 135)
        } else {
            (
                mix_channel(25, 34, vertical).saturating_sub(vignette as u8),
                mix_channel(108, 128, vertical).saturating_sub(vignette as u8),
                mix_channel(132, 122, vertical).saturating_sub(vignette as u8),
            )
        };
        let color = ((0xFF_u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
        let pixel: &mut [u8; 4] = chunk.try_into().unwrap();
        *pixel = color.to_le_bytes();
    }
}

fn mix_channel(start: u8, end: u8, amount: u32) -> u8 {
    let start = start as u32;
    let end = end as u32;
    (((start * (255 - amount)) + (end * amount)) / 255) as u8
}

fn pick_mime(mime_types: &[String]) -> Option<String> {
    SUPPORTED_MIME_TYPES
        .iter()
        .find(|supported| mime_types.iter().any(|mime| mime == **supported))
        .map(|mime| (*mime).to_owned())
}

fn log_mime_types(prefix: &str, mime_types: &[String]) {
    if mime_types.is_empty() {
        eprintln!("[dnd-demo] {prefix}: offered mimes=<none>");
    } else {
        eprintln!(
            "[dnd-demo] {prefix}: offered mimes={}",
            mime_types.join(", ")
        );
    }
}

fn log_payload_preview(data: &[u8]) {
    const MAX_PREVIEW_BYTES: usize = 4096;
    let preview = if data.len() > MAX_PREVIEW_BYTES {
        &data[..MAX_PREVIEW_BYTES]
    } else {
        data
    };
    eprintln!(
        "[dnd-demo] drop-in: payload preview {:?}",
        String::from_utf8_lossy(preview)
    );
    if data.len() > MAX_PREVIEW_BYTES {
        eprintln!(
            "[dnd-demo] drop-in: payload preview truncated total_bytes={}",
            data.len()
        );
    }
}
