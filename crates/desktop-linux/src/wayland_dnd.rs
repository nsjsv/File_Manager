mod handlers;

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use smithay_client_toolkit::data_device_manager::{
    data_device::DataDevice, data_offer::DragOffer, data_source::DragSource, DataDeviceManagerState,
};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle, PostAction};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::registry::RegistryState;
use smithay_client_toolkit::seat::SeatState;
use thiserror::Error;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{
    wl_data_device::WlDataDevice, wl_data_device_manager::DndAction, wl_pointer::WlPointer,
    wl_seat::WlSeat, wl_surface,
};
use wayland_client::{Connection, Proxy, QueueHandle};

use crate::file_clipboard::{
    parse_file_uri_list, parse_gnome_copied_files, serialize_file_uri_list,
    serialize_gnome_copied_files, FileClipboardOperation, FileClipboardPayloadError,
    FileClipboardSelection, GNOME_COPIED_FILES_MIME, URI_LIST_MIME,
};

const SUPPORTED_MIME_TYPES: &[&str] = &[
    INTERNAL_FILE_DRAG_MIME,
    GNOME_COPIED_FILES_MIME,
    URI_LIST_MIME,
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
];
const INTERNAL_FILE_DRAG_MIME: &str = "application/x-file-manager-internal-dnd";
const DRAG_REQUEST_TTL: Duration = Duration::from_millis(750);
static NEXT_CONTROLLER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WaylandDndWindowHandle {
    pub display_ptr: usize,
    pub surface_ptr: usize,
}

impl WaylandDndWindowHandle {
    pub fn new(display_ptr: usize, surface_ptr: usize) -> Self {
        Self {
            display_ptr,
            surface_ptr,
        }
    }
}

#[derive(Debug, Clone)]
pub enum WaylandDndEvent {
    FilesDropped(WaylandDndFileDrop),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct WaylandDndFileDrop {
    pub selection: FileClipboardSelection,
    pub origin: WaylandDndDropOrigin,
    pub position: Option<WaylandDndDropPosition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaylandDndDropOrigin {
    External,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaylandDndDropPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug)]
pub enum WaylandDndCommand {
    StartFileDrag(Vec<PathBuf>),
}

#[derive(Debug)]
pub struct WaylandDndController {
    id: u64,
    command_sender: UnboundedSender<WaylandDndCommand>,
    command_receiver: Mutex<Option<UnboundedReceiver<WaylandDndCommand>>>,
}

impl WaylandDndController {
    pub fn new() -> Arc<Self> {
        let (command_sender, command_receiver) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(Self {
            id: NEXT_CONTROLLER_ID.fetch_add(1, Ordering::Relaxed),
            command_sender,
            command_receiver: Mutex::new(Some(command_receiver)),
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn start_file_drag(&self, paths: Vec<PathBuf>) -> Result<(), WaylandDndCommandError> {
        self.command_sender
            .send(WaylandDndCommand::StartFileDrag(paths))
            .map_err(|_| WaylandDndCommandError::WorkerStopped)
    }

    fn take_command_receiver(&self) -> Option<UnboundedReceiver<WaylandDndCommand>> {
        self.command_receiver.lock().ok()?.take()
    }
}

#[derive(Debug, Error)]
pub enum WaylandDndCommandError {
    #[error("Wayland drag-and-drop worker is not running")]
    WorkerStopped,
}

#[derive(Debug, Error)]
pub enum WaylandDndError {
    #[error("could not start Wayland drag-and-drop worker: {source}")]
    ThreadSpawn {
        #[source]
        source: std::io::Error,
    },
    #[error("could not initialize Wayland drag-and-drop at {stage}: {details}")]
    Setup {
        stage: &'static str,
        details: String,
    },
    #[error("Wayland drag payload for {mime} is not UTF-8: {source}")]
    PayloadUtf8 {
        mime: String,
        #[source]
        source: std::str::Utf8Error,
    },
    #[error("Wayland drag payload for {mime} is invalid: {source}")]
    Payload {
        mime: String,
        #[source]
        source: FileClipboardPayloadError,
    },
}

pub fn spawn_wayland_file_dnd(
    window_handle: WaylandDndWindowHandle,
    controller: Arc<WaylandDndController>,
    event_sender: UnboundedSender<WaylandDndEvent>,
    shutdown_receiver: mpsc::Receiver<()>,
) -> Result<thread::JoinHandle<()>, WaylandDndError> {
    thread::Builder::new()
        .name("wayland-file-dnd".to_owned())
        .spawn(move || {
            if let Err(error) = run_wayland_file_dnd(
                window_handle,
                controller,
                event_sender.clone(),
                shutdown_receiver,
            ) {
                let _ = event_sender.send(WaylandDndEvent::Failed(error.to_string()));
            }
        })
        .map_err(|source| WaylandDndError::ThreadSpawn { source })
}

struct WaylandFileDnd {
    registry_state: RegistryState,
    seat_state: SeatState,
    data_device_state: DataDeviceManagerState,
    surface: wl_surface::WlSurface,
    seat_objects: Vec<SeatObject>,
    drag_sources: Vec<DragSession>,
    drop_reads: Vec<DropRead>,
    drop_is_over_surface: bool,
    drop_position: Option<WaylandDndDropPosition>,
    loop_handle: LoopHandle<'static, WaylandFileDnd>,
    pending_file_drag: Option<PendingFileDrag>,
    active_left_press: Option<PointerPress>,
    event_sender: UnboundedSender<WaylandDndEvent>,
}

struct SeatObject {
    seat: WlSeat,
    pointer: Option<WlPointer>,
    data_device: DataDevice,
}

struct DragSession {
    source: DragSource,
    payload: DragPayload,
    selected_action: DndAction,
}

struct PendingFileDrag {
    paths: Vec<PathBuf>,
    requested_at: Instant,
}

#[derive(Clone)]
struct PointerPress {
    pointer: WlPointer,
    surface: wl_surface::WlSurface,
    serial: u32,
}

struct DropRead {
    offer: DragOffer,
    mime_type: String,
    data: Vec<u8>,
    position: Option<WaylandDndDropPosition>,
}

fn run_wayland_file_dnd(
    window_handle: WaylandDndWindowHandle,
    controller: Arc<WaylandDndController>,
    event_sender: UnboundedSender<WaylandDndEvent>,
    shutdown_receiver: mpsc::Receiver<()>,
) -> Result<(), WaylandDndError> {
    let mut command_receiver = controller
        .take_command_receiver()
        .ok_or_else(|| setup_error("command-receiver", "command receiver was already taken"))?;
    let (conn, surface) = unsafe { wayland_connection_from_raw_handle(window_handle) }?;
    let (globals, event_queue) =
        registry_queue_init(&conn).map_err(|error| setup_error("registry", error))?;
    let qh = event_queue.handle();
    let mut event_loop = EventLoop::try_new().map_err(|error| setup_error("event-loop", error))?;
    WaylandSource::new(conn, event_queue)
        .insert(event_loop.handle())
        .map_err(|error| setup_error("wayland-source", error))?;

    let data_device_state = DataDeviceManagerState::bind(&globals, &qh)
        .map_err(|error| setup_error("data-device-manager", format!("{error:?}")))?;
    let mut dnd = WaylandFileDnd {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        data_device_state,
        surface,
        seat_objects: Vec::new(),
        drag_sources: Vec::new(),
        drop_reads: Vec::new(),
        drop_is_over_surface: false,
        drop_position: None,
        loop_handle: event_loop.handle(),
        pending_file_drag: None,
        active_left_press: None,
        event_sender,
    };

    tracing::debug!(
        controller_id = controller.id(),
        "Wayland file drag-and-drop worker started"
    );

    loop {
        match shutdown_receiver.try_recv() {
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {}
        }
        dnd.receive_commands(&mut command_receiver, &qh);
        event_loop
            .dispatch(Duration::from_millis(16), &mut dnd)
            .map_err(|error| setup_error("dispatch", error))?;
        dnd.receive_commands(&mut command_receiver, &qh);
    }

    Ok(())
}

unsafe fn wayland_connection_from_raw_handle(
    window_handle: WaylandDndWindowHandle,
) -> Result<(Connection, wl_surface::WlSurface), WaylandDndError> {
    // raw-window-handle 的指针生命周期由 Iced/winit 拥有，这里只在同一 wl_display 上创建代理。
    let backend = unsafe { Backend::from_foreign_display(window_handle.display_ptr as *mut _) };
    let conn = Connection::from_backend(backend);
    let surface_id = unsafe {
        ObjectId::from_ptr(
            wl_surface::WlSurface::interface(),
            window_handle.surface_ptr as *mut _,
        )
    }
    .map_err(|error| setup_error("surface-id", error))?;
    let surface = wl_surface::WlSurface::from_id(&conn, surface_id)
        .map_err(|error| setup_error("surface-proxy", error))?;
    Ok((conn, surface))
}

fn setup_error(stage: &'static str, error: impl std::fmt::Display) -> WaylandDndError {
    WaylandDndError::Setup {
        stage,
        details: error.to_string(),
    }
}

impl WaylandFileDnd {
    fn receive_commands(
        &mut self,
        command_receiver: &mut UnboundedReceiver<WaylandDndCommand>,
        qh: &QueueHandle<Self>,
    ) {
        while let Ok(command) = command_receiver.try_recv() {
            match command {
                WaylandDndCommand::StartFileDrag(paths) => {
                    tracing::debug!(
                        path_count = paths.len(),
                        "Wayland file drag request received"
                    );
                    self.pending_file_drag = Some(PendingFileDrag {
                        paths,
                        requested_at: Instant::now(),
                    });
                    self.start_pending_file_drag(qh);
                }
            }
        }
        self.expire_pending_file_drag();
    }

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
        self.seat_objects.len() - 1
    }

    fn data_device_for(&self, data_device: &WlDataDevice) -> Option<&DataDevice> {
        self.seat_objects
            .iter()
            .find(|seat_object| seat_object.data_device.inner() == data_device)
            .map(|seat_object| &seat_object.data_device)
    }

    fn remember_pointer_press(
        &mut self,
        qh: &QueueHandle<Self>,
        pointer: &WlPointer,
        surface: &wl_surface::WlSurface,
        serial: u32,
    ) {
        self.active_left_press = Some(PointerPress {
            pointer: pointer.clone(),
            surface: surface.clone(),
            serial,
        });
        tracing::debug!(serial, "Wayland pointer press serial captured");
        self.start_pending_file_drag(qh);
    }

    fn clear_pointer_press(&mut self, serial: u32) {
        tracing::debug!(serial, "Wayland pointer release serial observed");
        self.active_left_press = None;
    }

    fn start_pending_file_drag(&mut self, qh: &QueueHandle<Self>) {
        let Some(press) = self.active_left_press.clone() else {
            return;
        };
        if press.surface != self.surface {
            return;
        }
        let Some(request) = self.pending_file_drag.take() else {
            return;
        };
        if request.paths.is_empty() {
            tracing::debug!("Wayland file drag request ignored because it has no paths");
            return;
        }
        if request.requested_at.elapsed() > DRAG_REQUEST_TTL {
            tracing::debug!("Wayland file drag request expired before a usable press serial");
            return;
        }
        let Some(seat_index) = self
            .seat_objects
            .iter()
            .position(|seat_object| seat_object.pointer.as_ref() == Some(&press.pointer))
        else {
            tracing::debug!("Wayland file drag request ignored because no seat owns the pointer");
            return;
        };

        let payload = DragPayload::new(&request.paths);
        let source = self.data_device_state.create_drag_and_drop_source(
            qh,
            SUPPORTED_MIME_TYPES.to_vec(),
            DndAction::Move,
        );
        source.start_drag(
            &self.seat_objects[seat_index].data_device,
            &press.surface,
            None,
            press.serial,
        );
        tracing::debug!(
            serial = press.serial,
            path_count = request.paths.len(),
            "Wayland file drag started"
        );
        self.drag_sources.push(DragSession {
            source,
            payload,
            selected_action: DndAction::empty(),
        });
    }

    fn expire_pending_file_drag(&mut self) {
        if self
            .pending_file_drag
            .as_ref()
            .is_some_and(|request| request.requested_at.elapsed() > DRAG_REQUEST_TTL)
        {
            tracing::debug!("Wayland file drag request expired");
            self.pending_file_drag = None;
        }
    }

    fn register_drop_read(&mut self, offer: DragOffer, mime_type: String) {
        let read_pipe = match offer.receive(mime_type.clone()) {
            Ok(read_pipe) => read_pipe,
            Err(error) => {
                let _ = self.event_sender.send(WaylandDndEvent::Failed(format!(
                    "could not receive Wayland drag payload for {mime_type}: {error:?}"
                )));
                offer.finish();
                offer.destroy();
                return;
            }
        };

        self.drop_reads.push(DropRead {
            offer: offer.clone(),
            mime_type,
            data: Vec::new(),
            position: self.drop_position,
        });

        let offer_key = offer.clone();
        if let Err(error) = self
            .loop_handle
            .insert_source(read_pipe, move |_, file, state| {
                state.read_drop_payload(&offer_key, file)
            })
        {
            self.drop_reads.retain(|read| read.offer != offer);
            let _ = self.event_sender.send(WaylandDndEvent::Failed(format!(
                "could not register Wayland drag payload reader: {error}"
            )));
            offer.finish();
            offer.destroy();
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
                let _ = self.event_sender.send(WaylandDndEvent::Failed(format!(
                    "could not read Wayland drag payload: {error}"
                )));
                drop_read.offer.finish();
                drop_read.offer.destroy();
                PostAction::Remove
            }
        }
    }

    fn finish_drop_read(&mut self, drop_read: DropRead) {
        match parse_drop_selection(&drop_read.mime_type, &drop_read.data) {
            Ok(parsed_drop) => {
                let _ = self
                    .event_sender
                    .send(WaylandDndEvent::FilesDropped(WaylandDndFileDrop {
                        selection: parsed_drop.selection,
                        origin: parsed_drop.origin,
                        position: drop_read.position,
                    }));
            }
            Err(error) => {
                let _ = self
                    .event_sender
                    .send(WaylandDndEvent::Failed(error.to_string()));
            }
        }
        drop_read.offer.finish();
        drop_read.offer.destroy();
    }

    fn remove_drag_source(
        &mut self,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
    ) {
        self.drag_sources
            .retain(|drag_session| drag_session.source.inner() != source);
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

struct DragPayload {
    internal_file_drag: String,
    text_uri_list: String,
    gnome_copied_files: String,
}

impl DragPayload {
    fn new(paths: &[PathBuf]) -> Self {
        let uri_list = serialize_file_uri_list(paths);
        let text_uri_list = if uri_list.is_empty() {
            String::new()
        } else {
            format!("{}\r\n", uri_list.replace('\n', "\r\n"))
        };
        let selection = FileClipboardSelection::new(FileClipboardOperation::Move, paths.to_vec());
        Self {
            internal_file_drag: text_uri_list.clone(),
            text_uri_list,
            gnome_copied_files: serialize_gnome_copied_files(&selection),
        }
    }

    fn for_mime(&self, mime: &str) -> Option<&str> {
        match mime {
            INTERNAL_FILE_DRAG_MIME => Some(&self.internal_file_drag),
            URI_LIST_MIME => Some(&self.text_uri_list),
            GNOME_COPIED_FILES_MIME => Some(&self.gnome_copied_files),
            "text/plain;charset=utf-8" | "UTF8_STRING" | "text/plain" => Some(&self.text_uri_list),
            _ => None,
        }
    }
}

struct ParsedDropSelection {
    selection: FileClipboardSelection,
    origin: WaylandDndDropOrigin,
}

fn parse_drop_selection(
    mime_type: &str,
    data: &[u8],
) -> Result<ParsedDropSelection, WaylandDndError> {
    let payload = std::str::from_utf8(data).map_err(|source| WaylandDndError::PayloadUtf8 {
        mime: mime_type.to_owned(),
        source,
    })?;
    let (paths, origin) = match mime_type {
        INTERNAL_FILE_DRAG_MIME => (
            parse_file_uri_list(payload).map_err(|source| WaylandDndError::Payload {
                mime: mime_type.to_owned(),
                source,
            })?,
            WaylandDndDropOrigin::Internal,
        ),
        GNOME_COPIED_FILES_MIME => parse_gnome_copied_files(payload)
            .map(|selection| selection.paths)
            .map_err(|source| WaylandDndError::Payload {
                mime: mime_type.to_owned(),
                source,
            })
            .map(|paths| (paths, WaylandDndDropOrigin::External))?,
        URI_LIST_MIME | "text/plain;charset=utf-8" | "UTF8_STRING" | "text/plain" => {
            parse_file_uri_list(payload)
                .map_err(|source| WaylandDndError::Payload {
                    mime: mime_type.to_owned(),
                    source,
                })
                .map(|paths| (paths, WaylandDndDropOrigin::External))?
        }
        _ => (Vec::new(), WaylandDndDropOrigin::External),
    };
    Ok(ParsedDropSelection {
        selection: FileClipboardSelection::new(FileClipboardOperation::Copy, paths),
        origin,
    })
}

fn pick_mime(mime_types: &[String]) -> Option<String> {
    SUPPORTED_MIME_TYPES
        .iter()
        .find(|supported| mime_types.iter().any(|mime| mime == **supported))
        .map(|mime| (*mime).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_payload_uses_move_operation_and_uri_list_line_endings() {
        let paths = vec![PathBuf::from("/tmp/a b"), PathBuf::from("/tmp/c")];

        let payload = DragPayload::new(&paths);

        assert_eq!(
            payload.text_uri_list,
            "file:///tmp/a%20b\r\nfile:///tmp/c\r\n"
        );
        assert_eq!(
            payload.gnome_copied_files,
            "cut\nfile:///tmp/a%20b\nfile:///tmp/c"
        );
    }

    #[test]
    fn drop_payload_forces_copy_even_if_gnome_payload_says_cut() {
        let selection =
            parse_drop_selection(GNOME_COPIED_FILES_MIME, b"cut\nfile:///tmp/source").unwrap();

        assert_eq!(selection.selection.operation, FileClipboardOperation::Copy);
        assert_eq!(
            selection.selection.paths,
            vec![PathBuf::from("/tmp/source")]
        );
        assert_eq!(selection.origin, WaylandDndDropOrigin::External);
    }

    #[test]
    fn internal_drag_payload_marks_drop_origin_internal() {
        let selection =
            parse_drop_selection(INTERNAL_FILE_DRAG_MIME, b"file:///tmp/source\r\n").unwrap();

        assert_eq!(selection.selection.operation, FileClipboardOperation::Copy);
        assert_eq!(
            selection.selection.paths,
            vec![PathBuf::from("/tmp/source")]
        );
        assert_eq!(selection.origin, WaylandDndDropOrigin::Internal);
    }

    #[test]
    fn controller_sends_file_drag_command_to_worker_receiver() {
        let controller = WaylandDndController::new();
        let path = PathBuf::from("/tmp/source");

        controller.start_file_drag(vec![path.clone()]).unwrap();

        let mut command_receiver = controller.take_command_receiver().unwrap();
        let command = command_receiver.try_recv().unwrap();
        match command {
            WaylandDndCommand::StartFileDrag(paths) => assert_eq!(paths, vec![path]),
        }
    }
}
