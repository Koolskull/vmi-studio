//! Pen pressure from the operating system.
//!
//! [`pressure`] reflects the pointing device that spoke last. A pen reports
//! its tip force, 0 once it is lifted or leaves proximity, and stays the last
//! device until a mouse or trackpad moves, which clears the value to `None`
//! so the window presses at 1 again. The window reads the value once per
//! frame. Every listener below runs off the UI thread and parks the value in
//! atomics, and none of them panics when its display server, extension or
//! tablet is missing: it prints one `tablet:` line and leaves `None` alone.
//!
//! * Windows: winit turns `WM_POINTER` pen input into touches with
//!   `Force::Normalized`, which egui exposes as `Event::Touch { force }`.
//!   The window prefers that force, and a mouse never produces a touch, so
//!   the last-device rule holds there without this module.
//! * macOS: winit hands egui nothing for a tablet, so an AppKit local event
//!   monitor reads `pressure` off tablet-point and left-mouse events. Huion
//!   and XP-Pen drivers post plain mouse events whose pressure is fractional,
//!   which counts as a pen too.
//! * Linux, X11 session (and XWayland): winit never decodes XInput2
//!   valuators, so a second X connection selects raw motion and button events
//!   on the root window and reads the valuator labelled `Abs Pressure` of the
//!   device that sent them, scaled by the range the device advertises.
//! * Linux, Wayland session: winit has no tablet-v2 support, so a listener on
//!   winit's own `wl_display` binds `zwp_tablet_manager_v2` from a second
//!   event queue. See the pointer-emulation caveat on the Linux `platform`
//!   module.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static PRESSURE: AtomicU32 = AtomicU32::new(0x3F80_0000); // 1.0f32
static SEEN: AtomicBool = AtomicBool::new(false);
/// Milliseconds of the last pen report. A mouse copy inside this window is
/// the tablet's own pointer, not a reason to drop the tip force.
static PEN_AT: AtomicU64 = AtomicU64::new(0);
const PEN_HOLD_MS: u64 = 150;

fn pen_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// The latest pen pressure, or `None` while no pen has spoken, or since a
/// mouse spoke after it.
pub fn pressure() -> Option<f32> {
    if SEEN.load(Ordering::Relaxed) {
        Some(f32::from_bits(PRESSURE.load(Ordering::Relaxed)))
    } else {
        None
    }
}

/// A pen spoke: park its pressure, clamped to 0..=1. NaN presses fully.
#[cfg_attr(not(all(feature = "window", unix)), allow(dead_code))]
pub(crate) fn report(value: f32) {
    let value = if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        1.0
    };
    PRESSURE.store(value.to_bits(), Ordering::Relaxed);
    PEN_AT.store(pen_millis(), Ordering::Relaxed);
    SEEN.store(true, Ordering::Relaxed);
}

/// A device without a pressure axis spoke: forget the pen, so the window is
/// back to a mouse at full pressure instead of a lifted pen's 0.
/// A report from the last 150 ms stays. Huion posts a mouse copy of the same
/// tip, and dropping it makes the first dabs full pressure.
#[cfg_attr(not(all(feature = "window", unix)), allow(dead_code))]
pub(crate) fn clear() {
    let last = PEN_AT.load(Ordering::Relaxed);
    let now = pen_millis();
    if last != 0 && now.saturating_sub(last) < PEN_HOLD_MS {
        return;
    }
    SEEN.store(false, Ordering::Relaxed);
}

/// Tests that want a mouse to win call this before `clear`.
#[cfg(test)]
pub(crate) fn expire_pen_hold() {
    PEN_AT.store(0, Ordering::Relaxed);
}

/// The tablet listener is done. Pressure goes back to a mouse immediately.
#[cfg_attr(
    not(all(feature = "window", unix, not(target_os = "macos"))),
    allow(dead_code)
)]
pub(crate) fn abandon() {
    PEN_AT.store(0, Ordering::Relaxed);
    SEEN.store(false, Ordering::Relaxed);
}

/// Start listening. Safe to call once at window creation. The creation
/// context names the display server, which picks the listener.
#[cfg(feature = "window")]
pub fn install(cc: &eframe::CreationContext<'_>) {
    platform::install(cc);
}

/// Pure arithmetic shared by the listeners, kept free of any display so it
/// can be tested anywhere.
#[cfg_attr(
    not(all(feature = "window", unix, not(target_os = "macos"))),
    allow(dead_code)
)]
mod axis {
    /// One valuator as XInput2 describes it.
    pub(crate) struct Axis {
        pub number: u16,
        pub label: Option<String>,
        pub min: f64,
        pub max: f64,
        /// The value at the time of the query.
        pub value: f64,
    }

    /// The label libinput, xf86-input-wacom, evdev and XWayland all give the
    /// pressure axis.
    pub(crate) const PRESSURE_LABEL: &str = "Abs Pressure";

    /// The axis that carries pressure. Failing a label, a device with three
    /// or more axes keeps pressure third, but only a real range counts: a
    /// scroll axis there sits at -1..-1, and a relative axis is never
    /// pressure.
    pub(crate) fn pressure_axis(axes: &[Axis]) -> Option<&Axis> {
        axes.iter()
            .find(|axis| axis.label.as_deref() == Some(PRESSURE_LABEL))
            .or_else(|| {
                if axes.len() < 3 {
                    return None;
                }
                axes.iter().find(|axis| {
                    axis.number == 2
                        && axis.max > 1.0
                        && !axis
                            .label
                            .as_deref()
                            .is_some_and(|label| label.starts_with("Rel "))
                })
            })
    }

    /// XInput2 FP3232: a signed integral part plus an unsigned 32-bit fraction.
    pub(crate) fn fp3232(integral: i32, frac: u32) -> f64 {
        f64::from(integral) + f64::from(frac) / 4_294_967_296.0
    }

    /// Where `axis` sits in a raw event's packed value list, if the mask
    /// names it. Bit `n` of word `w` stands for axis `32 * w + n`; values are
    /// packed in rising axis order.
    pub(crate) fn valuator_index(mask: &[u32], axis: u16) -> Option<usize> {
        let (word, bit) = (usize::from(axis / 32), axis % 32);
        let current = *mask.get(word)?;
        if current & (1 << bit) == 0 {
            return None;
        }
        let before: u32 = mask[..word].iter().map(|w| w.count_ones()).sum();
        let below = (current & ((1u32 << bit) - 1)).count_ones();
        Some((before + below) as usize)
    }

    /// `value` scaled across the device's range into 0..=1, clamped. A device
    /// without a usable range (max no larger than min) is taken as 0..=1
    /// already. NaN presses fully, like a mouse.
    pub(crate) fn normalise(value: f64, min: f64, max: f64) -> f32 {
        let scaled = if max > min {
            (value - min) / (max - min)
        } else {
            value
        };
        if scaled.is_finite() {
            scaled.clamp(0.0, 1.0) as f32
        } else {
            1.0
        }
    }

    /// The pressure state of one device that has a pressure axis.
    pub(crate) struct PenAxis {
        pub number: u16,
        pub min: f64,
        pub max: f64,
        pub last: f32,
    }

    impl PenAxis {
        pub(crate) fn new(axis: &Axis) -> Self {
            Self {
                number: axis.number,
                min: axis.min,
                max: axis.max,
                last: normalise(axis.value, axis.min, axis.max),
            }
        }

        /// Folds one raw event's valuators in and returns the pressure to
        /// publish. An event that lacks the axis keeps the device's last value.
        pub(crate) fn update(&mut self, mask: &[u32], values: &[f64]) -> f32 {
            if let Some(value) =
                valuator_index(mask, self.number).and_then(|index| values.get(index))
            {
                self.last = normalise(*value, self.min, self.max);
            }
            self.last
        }
    }
}

#[cfg(all(feature = "window", target_os = "macos"))]
mod platform {
    use std::ptr::NonNull;
    use std::sync::Once;

    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventSubtype, NSEventType};

    pub fn install(_cc: &eframe::CreationContext<'_>) {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let mask = NSEventMask::LeftMouseDown
                | NSEventMask::LeftMouseDragged
                | NSEventMask::LeftMouseUp
                | NSEventMask::MouseMoved
                | NSEventMask::TabletPoint
                | NSEventMask::TabletProximity;
            let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
                // SAFETY: AppKit hands us a live event for the duration of the call,
                // and the mask above only admits the event kinds `observe` reads.
                unsafe { observe(event.as_ref()) };
                event.as_ptr()
            });
            // SAFETY: called on the main thread at startup; the monitor lives for the process.
            let monitor =
                unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) };
            std::mem::forget(monitor);
            std::mem::forget(block);
            eprintln!("tablet: AppKit event monitor");
        });
    }

    /// # Safety
    ///
    /// `event` must be a left-mouse, mouse-moved, tablet-point or
    /// tablet-proximity event. AppKit raises on `pressure` outside mouse
    /// down, up and dragged and tablet-point events, and on
    /// `isEnteringProximity` outside proximity events, so each is only read
    /// where it is defined.
    unsafe fn observe(event: &NSEvent) {
        let kind = event.r#type();
        if kind == NSEventType::TabletPoint {
            super::report(event.pressure());
            return;
        }
        if kind == NSEventType::TabletProximity {
            if !event.isEnteringProximity() {
                super::report(0.0);
            }
            return;
        }
        // The rest are mouse events, which always carry a subtype.
        let subtype = event.subtype();
        if subtype == NSEventSubtype::TabletProximity {
            if !event.isEnteringProximity() {
                super::report(0.0);
            }
        } else if kind == NSEventType::MouseMoved {
            // No pressure on a move. A pen moving without pressing is lifted;
            // a plain mouse move means the mouse took over.
            if subtype == NSEventSubtype::TabletPoint {
                super::report(0.0);
            } else if subtype == NSEventSubtype::MouseEvent {
                super::clear();
            }
        } else {
            // Left down, dragged or up. A pen comes through AppKit's tablet
            // subtype, or through a driver (Huion, XP-Pen) that posts mouse
            // events with fractional pressure; a plain mouse reads exactly 0 or 1.
            let pressure = event.pressure();
            let pen = subtype == NSEventSubtype::TabletPoint || (pressure > 0.0 && pressure < 1.0);
            if kind == NSEventType::LeftMouseUp {
                if pen {
                    super::report(0.0);
                } else if subtype == NSEventSubtype::MouseEvent {
                    super::clear();
                }
            } else if pen {
                super::report(pressure);
            } else if subtype == NSEventSubtype::MouseEvent {
                super::clear();
            }
        }
    }
}

#[cfg(all(feature = "window", unix, not(target_os = "macos")))]
mod platform {
    //! Linux and the BSDs. The X11 listener owns a second connection and sees
    //! raw events from every device, focus or not, which is also how XWayland
    //! shows a Wayland tablet to an X11 window. The Wayland listener has to
    //! live on winit's connection: a compositor only tells the client that
    //! owns the focused surface about a pen.
    //!
    //! Caveat, Wayland: once a client binds the tablet seat, mutter sends it
    //! pen events through tablet-v2 only (it never emulates a pointer for a
    //! pen), and KWin and the wlroots compositors stop emulating the pointer
    //! for it. winit does not read tablet-v2, so egui would stop seeing the
    //! pen move at all. The listener therefore watches its own `wl_pointer`:
    //! if a pen is in proximity and no pointer motion arrives alongside its
    //! first frames, it releases the tablet seat again, which hands KWin and
    //! wlroots back to pointer emulation at full pressure. Pressure stays
    //! live where the compositor emulates and reports alike (Smithay-based
    //! compositors such as COSMIC). Full pen support on Wayland needs the
    //! window to take positions and buttons from tablet-v2 itself.

    use std::error::Error;
    use std::sync::Once;

    pub fn install(cc: &eframe::CreationContext<'_>) {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| match server(cc) {
            Server::Wayland(display) => wayland::spawn(display),
            Server::X11 => x11::spawn(),
            Server::None => eprintln!("tablet: none (no display server handle)"),
        });
    }

    enum Server {
        X11,
        /// winit's `wl_display`, carried as an address so it can cross threads.
        Wayland(usize),
        None,
    }

    /// The display server behind the window, from its handle, else from the
    /// environment the way winit picks one.
    fn server(cc: &eframe::CreationContext<'_>) -> Server {
        use raw_window_handle::{HasDisplayHandle as _, RawDisplayHandle};
        match cc.display_handle().map(|handle| handle.as_raw()) {
            Ok(RawDisplayHandle::Wayland(handle)) => {
                Server::Wayland(handle.display.as_ptr() as usize)
            }
            Ok(RawDisplayHandle::Xlib(_)) | Ok(RawDisplayHandle::Xcb(_)) => Server::X11,
            _ => {
                let set =
                    |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
                if set("WAYLAND_DISPLAY") && !set("DISPLAY") {
                    // A Wayland session, but without winit's display there is no connection to join.
                    Server::None
                } else if set("DISPLAY") {
                    Server::X11
                } else {
                    Server::None
                }
            }
        }
    }

    /// Runs a listener on its own thread. Failure to start or a later error
    /// costs one line of stderr and nothing else.
    fn spawn(
        name: &'static str,
        run: impl FnOnce() -> Result<(), Box<dyn Error>> + Send + 'static,
    ) {
        let started = std::thread::Builder::new()
            .name(format!("tablet-{name}"))
            .spawn(move || {
                if let Err(err) = run() {
                    eprintln!("tablet: {name} listener stopped: {err}");
                }
            });
        if let Err(err) = started {
            eprintln!("tablet: none ({name} thread: {err})");
        }
    }

    pub(crate) mod x11 {
        use std::collections::HashMap;
        use std::error::Error;

        use x11rb::connection::Connection as _;
        use x11rb::protocol::xinput::{self, ConnectionExt as _, DeviceClassData, Fp3232};
        use x11rb::protocol::xproto::{Atom, ConnectionExt as _};
        use x11rb::protocol::Event;
        use x11rb::rust_connection::RustConnection;

        use crate::tablet::axis::{self, Axis, PenAxis};

        pub fn spawn() {
            super::spawn("XInput2", || Listener::open()?.run());
        }

        pub(crate) struct Listener {
            conn: RustConnection,
            devices: Devices,
        }

        impl Listener {
            /// Connects to `$DISPLAY`, asks for XI 2.3 and selects raw motion
            /// and button events from every device on the root window. Raw
            /// events reach a root-window selector whatever window has focus
            /// (and, from XI 2.1 on, through grabs).
            pub(crate) fn open() -> Result<Self, Box<dyn Error>> {
                let (conn, screen) = x11rb::connect(None)?;
                let root = conn.setup().roots.get(screen).ok_or("no such screen")?.root;
                let version = conn.xinput_xi_query_version(2, 3)?.reply()?;
                if version.major_version < 2 {
                    return Err(format!(
                        "XInput {}.{} has no raw events",
                        version.major_version, version.minor_version
                    )
                    .into());
                }
                let mask = xinput::XIEventMask::RAW_MOTION
                    | xinput::XIEventMask::RAW_BUTTON_PRESS
                    | xinput::XIEventMask::RAW_BUTTON_RELEASE
                    | xinput::XIEventMask::HIERARCHY
                    | xinput::XIEventMask::DEVICE_CHANGED;
                conn.xinput_xi_select_events(
                    root,
                    &[xinput::EventMask {
                        deviceid: xinput::Device::ALL.into(),
                        mask: vec![mask],
                    }],
                )?
                .check()?;
                eprintln!(
                    "tablet: XInput2 {}.{} pressure on {}",
                    version.major_version,
                    version.minor_version,
                    std::env::var("DISPLAY").unwrap_or_default()
                );
                Ok(Self {
                    conn,
                    devices: Devices::default(),
                })
            }

            fn run(mut self) -> Result<(), Box<dyn Error>> {
                loop {
                    let event = self.conn.wait_for_event()?;
                    self.handle(event);
                }
            }

            pub(crate) fn handle(&mut self, event: Event) {
                match event {
                    Event::XinputRawMotion(raw)
                    | Event::XinputRawButtonPress(raw)
                    | Event::XinputRawButtonRelease(raw) => self.devices.raw(&self.conn, &raw),
                    // Huion posts an empty hierarchy and a device-changed on
                    // almost every packet. Forgetting the cache there makes
                    // the listener query the server on the next sample and
                    // the first dabs of a thin line wait on that round trip.
                    // A device leaves the cache only when the event names it
                    // disabled or removed. Axis ranges do not change mid-stroke.
                    Event::XinputHierarchy(event) => self.devices.note_hierarchy(&event),
                    Event::XinputDeviceChanged(_) => {}
                    _ => {}
                }
            }

            #[cfg(test)]
            pub(crate) fn poll(&mut self) -> Result<Option<Event>, Box<dyn Error>> {
                Ok(self.conn.poll_for_event()?)
            }

            /// One line per device: its name and how its axes read.
            #[cfg(test)]
            pub(crate) fn describe(&mut self) -> Vec<String> {
                let mut lines = Vec::new();
                let reply = self
                    .conn
                    .xinput_xi_query_device(xinput::Device::ALL)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok());
                let Some(reply) = reply else {
                    return lines;
                };
                for info in &reply.infos {
                    let axes = self.devices.axes(&self.conn, info);
                    let listed: Vec<String> = axes
                        .iter()
                        .map(|axis| {
                            format!(
                                "{}:{:?} {}..{}",
                                axis.number, axis.label, axis.min, axis.max
                            )
                        })
                        .collect();
                    let pen = axis::pressure_axis(&axes).map(|axis| axis.number);
                    lines.push(format!(
                        "device {} {:?} pen axis {:?} axes [{}]",
                        info.deviceid,
                        String::from_utf8_lossy(&info.name),
                        pen,
                        listed.join(", ")
                    ));
                }
                lines
            }

            /// Pretends the server described `device` as a pen whose
            /// pressure sits on `axis`, so synthetic raw events can take the
            /// real decoding path.
            #[cfg(test)]
            pub(crate) fn pretend_pen(&mut self, device: u16, axis: &Axis) {
                self.devices.pens.insert(device, Some(PenAxis::new(axis)));
            }

            /// A device the server describes with axes but no pressure.
            #[cfg(test)]
            pub(crate) fn mouse(&mut self) -> Option<u16> {
                let reply = self
                    .conn
                    .xinput_xi_query_device(xinput::Device::ALL)
                    .ok()?
                    .reply()
                    .ok()?;
                for info in &reply.infos {
                    let axes = self.devices.axes(&self.conn, info);
                    if !axes.is_empty() && axis::pressure_axis(&axes).is_none() {
                        return Some(info.deviceid);
                    }
                }
                None
            }
        }

        /// What each source device's axes mean, learnt on first sight and
        /// forgotten when the server says the devices changed.
        #[derive(Default)]
        struct Devices {
            /// Per source device: its pressure axis, or `None` for a device
            /// without one.
            pens: HashMap<u16, Option<PenAxis>>,
            names: HashMap<Atom, Option<String>>,
        }

        impl Devices {
            fn raw(&mut self, conn: &RustConnection, raw: &xinput::RawButtonPressEvent) {
                // Selecting every device delivers one physical event twice:
                // once as the slave that made it and once as the master it
                // hangs off. The slave copy carries the same valuators and
                // also covers a floating device.
                if raw.deviceid != raw.sourceid {
                    return;
                }
                match self.pen(conn, raw.sourceid) {
                    // The server would not describe the device; the event tells nothing.
                    Err(()) => {}
                    // No pressure axis: a mouse, trackpad or tablet pad spoke.
                    Ok(None) => crate::tablet::clear(),
                    Ok(Some(pen)) => {
                        // The raw values sit in the device's own range, the one
                        // its valuator class advertises.
                        let values: Vec<f64> =
                            raw.axisvalues_raw.iter().map(|value| fp(*value)).collect();
                        crate::tablet::report(pen.update(&raw.valuator_mask, &values));
                    }
                }
            }

            /// The pressure axis of `device`, queried once. `Err` means the
            /// server would not say, so the event tells nothing.
            fn pen(
                &mut self,
                conn: &RustConnection,
                device: u16,
            ) -> Result<Option<&mut PenAxis>, ()> {
                if !self.pens.contains_key(&device) {
                    let pen = self.query(conn, device)?;
                    self.pens.insert(device, pen);
                }
                Ok(self.pens.get_mut(&device).and_then(Option::as_mut))
            }

            fn query(&mut self, conn: &RustConnection, device: u16) -> Result<Option<PenAxis>, ()> {
                let reply = conn
                    .xinput_xi_query_device(device)
                    .map_err(drop)?
                    .reply()
                    .map_err(drop)?;
                let Some(info) = reply.infos.iter().find(|info| info.deviceid == device) else {
                    return Ok(None);
                };
                let axes = self.axes(conn, info);
                let pen = axis::pressure_axis(&axes);
                let name = String::from_utf8_lossy(&info.name);
                match pen {
                    Some(axis) => eprintln!(
                        "tablet: device {device} {name:?}: pressure on axis {} ({}..{})",
                        axis.number, axis.min, axis.max
                    ),
                    None => eprintln!(
                        "tablet: device {device} {name:?}: no pressure axis, presses at 1"
                    ),
                }
                Ok(pen.map(PenAxis::new))
            }

            fn axes(&mut self, conn: &RustConnection, info: &xinput::XIDeviceInfo) -> Vec<Axis> {
                info.classes
                    .iter()
                    .filter_map(|class| match &class.data {
                        DeviceClassData::Valuator(valuator) => Some(Axis {
                            number: valuator.number,
                            label: self.name(conn, valuator.label),
                            min: fp(valuator.min),
                            max: fp(valuator.max),
                            value: fp(valuator.value),
                        }),
                        _ => None,
                    })
                    .collect()
            }

            fn name(&mut self, conn: &RustConnection, atom: Atom) -> Option<String> {
                if atom == x11rb::NONE {
                    return None;
                }
                self.names
                    .entry(atom)
                    .or_insert_with(|| {
                        let reply = conn.get_atom_name(atom).ok()?.reply().ok()?;
                        Some(String::from_utf8_lossy(&reply.name).into_owned())
                    })
                    .clone()
            }

            fn forget(&mut self, device: u16) {
                self.pens.remove(&device);
            }

            fn note_hierarchy(&mut self, event: &xinput::HierarchyEvent) {
                let drop_mask = u32::from(xinput::HierarchyMask::SLAVE_REMOVED)
                    | u32::from(xinput::HierarchyMask::DEVICE_DISABLED)
                    | u32::from(xinput::HierarchyMask::MASTER_REMOVED);
                for info in &event.infos {
                    if !info.enabled || u32::from(info.flags) & drop_mask != 0 {
                        self.forget(info.deviceid);
                    }
                }
            }
        }

        fn fp(value: Fp3232) -> f64 {
            axis::fp3232(value.integral, value.frac)
        }
    }

    pub(crate) mod wayland {
        use std::error::Error;

        use wayland_client::backend::Backend;
        use wayland_client::globals::{registry_queue_init, GlobalListContents};
        use wayland_client::protocol::{wl_pointer, wl_registry, wl_seat};
        use wayland_client::{
            delegate_noop, event_created_child, Connection, Dispatch, Proxy as _, QueueHandle,
            WEnum,
        };
        use wayland_protocols::wp::tablet::zv2::client::{
            zwp_tablet_manager_v2::ZwpTabletManagerV2,
            zwp_tablet_pad_dial_v2::ZwpTabletPadDialV2,
            zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
            zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
            zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
            zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
            zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
            zwp_tablet_tool_v2::{self, ZwpTabletToolV2},
            zwp_tablet_v2::{self, ZwpTabletV2},
        };

        use crate::tablet::axis;

        /// tablet-v2 reports pressure on this scale.
        const PRESSURE_MAX: f64 = 65535.0;
        /// Pen frames to wait for an emulated pointer before deciding the
        /// compositor sends none.
        pub(crate) const PROBE_FRAMES: u32 = 8;

        pub fn spawn(display: usize) {
            super::spawn("wayland tablet-v2", move || run(display));
        }

        fn run(display: usize) -> Result<(), Box<dyn Error>> {
            // SAFETY: `display` is winit's live `wl_display`, taken from the
            // window's display handle. winit holds it open for the life of its
            // event loop, which is the life of the process, and a guest backend
            // never closes it.
            let backend = unsafe { Backend::from_foreign_display(display as *mut _) };
            let conn = Connection::from_backend(backend);
            let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
            let qh = queue.handle();
            let manager: ZwpTabletManagerV2 = globals
                .bind(&qh, 1..=1, ())
                .map_err(|err| format!("no zwp_tablet_manager_v2 ({err})"))?;
            let seat: wl_seat::WlSeat = globals
                .bind(&qh, 1..=5, ())
                .map_err(|err| format!("no wl_seat ({err})"))?;
            let tablet_seat = manager.get_tablet_seat(&seat, &qh, ());
            let mut state = State {
                manager,
                tablet_seat,
                pointer: None,
                tablets: Vec::new(),
                pads: Vec::new(),
                tools: Vec::new(),
                pens: Pens::default(),
            };
            eprintln!("tablet: wayland tablet-v2");
            loop {
                queue.blocking_dispatch(&mut state)?;
            }
        }

        struct State {
            manager: ZwpTabletManagerV2,
            tablet_seat: ZwpTabletSeatV2,
            pointer: Option<wl_pointer::WlPointer>,
            tablets: Vec<ZwpTabletV2>,
            pads: Vec<ZwpTabletPadV2>,
            /// One proxy per tool in `pens`, in the same order.
            tools: Vec<ZwpTabletToolV2>,
            pens: Pens,
        }

        impl State {
            /// Hands the tablet seat back to the compositor.
            fn release(&mut self) {
                for tool in self.tools.drain(..) {
                    tool.destroy();
                }
                for tablet in self.tablets.drain(..) {
                    tablet.destroy();
                }
                for pad in self.pads.drain(..) {
                    pad.destroy();
                }
                self.tablet_seat.destroy();
                self.manager.destroy();
            }
        }

        /// What a tool said, stripped of the protocol.
        #[derive(Clone, Copy, Debug)]
        pub(crate) enum PenEvent {
            HasPressure,
            ProximityIn,
            ProximityOut,
            Pressure(u32),
            Up,
            Frame,
        }

        /// The tools of the tablet seat and the pointer-emulation probe,
        /// kept free of the protocol so they can be tested anywhere.
        #[derive(Default)]
        pub(crate) struct Pens {
            tools: Vec<ToolState>,
            /// Tools currently in proximity of one of our surfaces.
            in_proximity: usize,
            /// Pen frames seen in proximity while emulation is still unknown.
            probe_frames: u32,
            pointer_seen: bool,
            /// Whether the compositor still emulates a pointer for the pen;
            /// `None` until the first pen frames have told.
            emulation: Option<bool>,
        }

        #[derive(Default)]
        struct ToolState {
            has_pressure: bool,
            in_proximity: bool,
            last: f32,
            /// Pressure gathered since the last frame.
            pending: Option<f32>,
        }

        impl Pens {
            /// The seat announced a tool; it takes the next index.
            pub(crate) fn added(&mut self) {
                self.tools.push(ToolState::default());
            }

            /// The compositor removed the tool at `index`.
            pub(crate) fn removed(&mut self, index: usize) {
                if index < self.tools.len() && self.tools.remove(index).in_proximity {
                    self.in_proximity = self.in_proximity.saturating_sub(1);
                }
            }

            /// Our `wl_pointer` entered, moved, clicked or scrolled.
            pub(crate) fn pointer(&mut self) {
                if self.in_proximity > 0 {
                    // The pen's own emulated pointer, or a mouse nudged while
                    // the pen hovers: the pen stays the last device.
                    self.pointer_seen = true;
                } else {
                    crate::tablet::clear();
                }
            }

            /// The tool at `index` spoke. Returns `true` once, when the probe
            /// has found a compositor without pointer emulation and the
            /// tablet seat should be handed back; the tools are forgotten
            /// with it.
            pub(crate) fn tool(&mut self, index: usize, event: PenEvent) -> bool {
                let Some(tool) = self.tools.get_mut(index) else {
                    return false;
                };
                match event {
                    PenEvent::HasPressure => tool.has_pressure = true,
                    PenEvent::ProximityIn => {
                        if !tool.in_proximity {
                            tool.in_proximity = true;
                            self.in_proximity += 1;
                        }
                        tool.pending = Some(0.0);
                        if self.emulation.is_none() {
                            self.probe_frames = 0;
                            self.pointer_seen = false;
                        }
                    }
                    PenEvent::ProximityOut => {
                        if tool.in_proximity {
                            tool.in_proximity = false;
                            self.in_proximity = self.in_proximity.saturating_sub(1);
                        }
                        tool.pending = Some(0.0);
                    }
                    PenEvent::Pressure(raw) => {
                        tool.pending = Some(axis::normalise(f64::from(raw), 0.0, PRESSURE_MAX));
                    }
                    PenEvent::Up => tool.pending = Some(0.0),
                    PenEvent::Frame => {
                        if let Some(value) = tool.pending.take() {
                            tool.last = value;
                        }
                        if tool.has_pressure {
                            crate::tablet::report(tool.last);
                        } else {
                            // A tool without a pressure axis (a tablet mouse or
                            // lens cursor) counts as a mouse.
                            crate::tablet::clear();
                        }
                        if tool.in_proximity {
                            return self.probe();
                        }
                    }
                }
                false
            }

            /// Decides, from the first pen frames, whether the compositor
            /// still emulates a pointer for the pen. Without emulation the
            /// window is blind to the pen (winit reads no tablet-v2), and on
            /// KWin and wlroots it was this binding that switched emulation
            /// off, so the tablet seat is handed back.
            fn probe(&mut self) -> bool {
                if self.emulation.is_some() {
                    return false;
                }
                self.probe_frames += 1;
                if self.pointer_seen {
                    self.emulation = Some(true);
                    eprintln!(
                        "tablet: wayland tablet-v2 pressure live, pointer emulation stays on"
                    );
                } else if self.probe_frames >= PROBE_FRAMES {
                    self.emulation = Some(false);
                    eprintln!(
                        "tablet: wayland tablet-v2 released: this compositor routes a pen through \
                         tablet-v2 only, which the window does not read yet, so the pen goes back \
                         to the pointer at full pressure"
                    );
                    self.tools.clear();
                    self.in_proximity = 0;
                    crate::tablet::abandon();
                    return true;
                }
                false
            }

            #[cfg(test)]
            pub(crate) fn emulation(&self) -> Option<bool> {
                self.emulation
            }
        }

        impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
            fn event(
                _: &mut Self,
                _: &wl_registry::WlRegistry,
                _: wl_registry::Event,
                _: &GlobalListContents,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }

        impl Dispatch<wl_seat::WlSeat, ()> for State {
            fn event(
                state: &mut Self,
                seat: &wl_seat::WlSeat,
                event: wl_seat::Event,
                _: &(),
                _: &Connection,
                qh: &QueueHandle<Self>,
            ) {
                if let wl_seat::Event::Capabilities {
                    capabilities: WEnum::Value(capabilities),
                } = event
                {
                    let has_pointer = capabilities.contains(wl_seat::Capability::Pointer);
                    if has_pointer && state.pointer.is_none() {
                        state.pointer = Some(seat.get_pointer(qh, ()));
                    } else if !has_pointer {
                        if let Some(pointer) = state.pointer.take() {
                            if pointer.version() >= 3 {
                                pointer.release();
                            }
                        }
                    }
                }
            }
        }

        impl Dispatch<wl_pointer::WlPointer, ()> for State {
            fn event(
                state: &mut Self,
                _: &wl_pointer::WlPointer,
                event: wl_pointer::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                use wl_pointer::Event;
                if let Event::Enter { .. }
                | Event::Motion { .. }
                | Event::Button { .. }
                | Event::Axis { .. } = event
                {
                    state.pens.pointer();
                }
            }
        }

        impl Dispatch<ZwpTabletSeatV2, ()> for State {
            fn event(
                state: &mut Self,
                _: &ZwpTabletSeatV2,
                event: zwp_tablet_seat_v2::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                use zwp_tablet_seat_v2::Event;
                match event {
                    Event::TabletAdded { id } => state.tablets.push(id),
                    Event::ToolAdded { id } => {
                        state.tools.push(id);
                        state.pens.added();
                    }
                    Event::PadAdded { id } => state.pads.push(id),
                    _ => {}
                }
            }

            event_created_child!(State, ZwpTabletSeatV2, [
                zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
                zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
                zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
            ]);
        }

        impl Dispatch<ZwpTabletToolV2, ()> for State {
            fn event(
                state: &mut Self,
                proxy: &ZwpTabletToolV2,
                event: zwp_tablet_tool_v2::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                use zwp_tablet_tool_v2::{Capability, Event};
                let Some(index) = state.tools.iter().position(|tool| tool == proxy) else {
                    return;
                };
                let event = match event {
                    Event::Capability {
                        capability: WEnum::Value(Capability::Pressure),
                    } => PenEvent::HasPressure,
                    Event::ProximityIn { .. } => PenEvent::ProximityIn,
                    Event::ProximityOut => PenEvent::ProximityOut,
                    Event::Pressure { pressure } => PenEvent::Pressure(pressure),
                    Event::Up => PenEvent::Up,
                    Event::Frame { .. } => PenEvent::Frame,
                    Event::Removed => {
                        state.pens.removed(index);
                        state.tools.remove(index).destroy();
                        return;
                    }
                    _ => return,
                };
                if state.pens.tool(index, event) {
                    state.release();
                }
            }
        }

        impl Dispatch<ZwpTabletV2, ()> for State {
            fn event(
                state: &mut Self,
                proxy: &ZwpTabletV2,
                event: zwp_tablet_v2::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                if let zwp_tablet_v2::Event::Removed = event {
                    state.tablets.retain(|tablet| tablet != proxy);
                    proxy.destroy();
                }
            }
        }

        impl Dispatch<ZwpTabletPadV2, ()> for State {
            fn event(
                state: &mut Self,
                proxy: &ZwpTabletPadV2,
                event: zwp_tablet_pad_v2::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                if let zwp_tablet_pad_v2::Event::Removed = event {
                    state.pads.retain(|pad| pad != proxy);
                    proxy.destroy();
                }
            }

            event_created_child!(State, ZwpTabletPadV2, [
                zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
            ]);
        }

        impl Dispatch<ZwpTabletPadGroupV2, ()> for State {
            fn event(
                _: &mut Self,
                _: &ZwpTabletPadGroupV2,
                _: zwp_tablet_pad_group_v2::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }

            event_created_child!(State, ZwpTabletPadGroupV2, [
                zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
                zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
                zwp_tablet_pad_group_v2::EVT_DIAL_OPCODE => (ZwpTabletPadDialV2, ()),
            ]);
        }

        delegate_noop!(State: ignore ZwpTabletPadRingV2);
        delegate_noop!(State: ignore ZwpTabletPadStripV2);
        delegate_noop!(State: ignore ZwpTabletPadDialV2);
        delegate_noop!(State: ZwpTabletManagerV2);
    }
}

#[cfg(all(feature = "window", not(unix)))]
mod platform {
    /// Windows: winit already hands egui a pen as a touch with force, and a
    /// mouse never produces a touch, so there is nothing to listen to.
    pub fn install(_cc: &eframe::CreationContext<'_>) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::axis::{fp3232, normalise, pressure_axis, valuator_index, Axis, PenAxis};
    use super::*;

    /// The pressure statics are shared: tests that touch them take turns.
    static TURN: Mutex<()> = Mutex::new(());

    fn turn() -> std::sync::MutexGuard<'static, ()> {
        TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn axis(number: u16, label: Option<&str>, min: f64, max: f64) -> Axis {
        Axis {
            number,
            label: label.map(str::to_owned),
            min,
            max,
            value: min,
        }
    }

    #[test]
    fn nothing_reported_means_a_mouse_and_a_report_clamps() {
        let _turn = turn();
        expire_pen_hold();
        clear();
        assert_eq!(pressure(), None);
        report(1.7);
        assert_eq!(pressure(), Some(1.0));
        report(0.25);
        assert_eq!(pressure(), Some(0.25));
        report(f32::NAN);
        assert_eq!(pressure(), Some(1.0));
    }

    #[test]
    fn a_mouse_after_a_lifted_pen_clears_back_to_full_pressure() {
        let _turn = turn();
        report(0.0);
        assert_eq!(pressure(), Some(0.0));
        // The tablet's pointer copy arrives in the same breath as the pen.
        clear();
        assert_eq!(pressure(), Some(0.0));
        expire_pen_hold();
        clear();
        assert_eq!(pressure(), None);
        report(0.5);
        assert_eq!(pressure(), Some(0.5));
        expire_pen_hold();
        clear();
        assert_eq!(pressure(), None);
    }

    #[test]
    fn fp3232_adds_the_fraction_to_the_signed_integral() {
        assert_eq!(fp3232(3, 0), 3.0);
        assert_eq!(fp3232(0, 1 << 31), 0.5);
        assert_eq!(fp3232(2, 1 << 30), 2.25);
        assert_eq!(fp3232(-1, 1 << 31), -0.5);
        assert_eq!(fp3232(-64, 0), -64.0);
    }

    #[test]
    fn valuator_index_counts_the_set_bits_below_the_axis() {
        assert_eq!(valuator_index(&[0b111], 0), Some(0));
        assert_eq!(valuator_index(&[0b111], 2), Some(2));
        assert_eq!(valuator_index(&[0b101], 2), Some(1));
        assert_eq!(valuator_index(&[0b101], 1), None);
        assert_eq!(valuator_index(&[0b111, 0b10], 33), Some(3));
        assert_eq!(valuator_index(&[0b111], 40), None);
        assert_eq!(valuator_index(&[], 0), None);
        assert_eq!(valuator_index(&[1 << 31], 31), Some(0));
    }

    #[test]
    fn normalise_scales_over_the_advertised_range_and_clamps() {
        assert!((normalise(32767.5, 0.0, 65535.0) - 0.5).abs() < 1e-6);
        assert_eq!(normalise(2047.0, 0.0, 2047.0), 1.0);
        assert_eq!(normalise(0.0, 0.0, 2047.0), 0.0);
        assert!((normalise(150.0, 100.0, 300.0) - 0.25).abs() < 1e-6);
        assert_eq!(normalise(-5.0, 0.0, 100.0), 0.0);
        assert_eq!(normalise(500.0, 0.0, 100.0), 1.0);
        assert!((normalise(0.3, 0.0, 0.0) - 0.3).abs() < 1e-6);
        assert_eq!(normalise(f64::NAN, 0.0, 1.0), 1.0);
        assert_eq!(normalise(f64::INFINITY, 0.0, 1.0), 1.0);
    }

    #[test]
    fn pressure_axis_prefers_the_label_and_falls_back_to_the_third_axis() {
        let libinput = [
            axis(0, Some("Abs X"), 0.0, 16777215.0),
            axis(1, Some("Abs Y"), 0.0, 16777215.0),
            axis(2, Some("Abs Pressure"), 0.0, 65535.0),
            axis(3, Some("Abs Tilt X"), -64.0, 63.0),
        ];
        assert_eq!(pressure_axis(&libinput).map(|a| a.number), Some(2));

        let wacom_odd_order = [
            axis(0, Some("Abs X"), 0.0, 21600.0),
            axis(1, Some("Abs Y"), 0.0, 13500.0),
            axis(2, Some("Abs Tilt X"), -64.0, 63.0),
            axis(3, Some("Abs Pressure"), 0.0, 2047.0),
        ];
        assert_eq!(pressure_axis(&wacom_odd_order).map(|a| a.number), Some(3));

        let mouse = [
            axis(0, Some("Rel X"), -1.0, -1.0),
            axis(1, Some("Rel Y"), -1.0, -1.0),
            axis(2, Some("Rel Horiz Scroll"), -1.0, -1.0),
            axis(3, Some("Rel Vert Scroll"), -1.0, -1.0),
        ];
        assert!(pressure_axis(&mouse).is_none());

        let unlabelled_tablet = [
            axis(0, None, 0.0, 32767.0),
            axis(1, None, 0.0, 32767.0),
            axis(2, None, 0.0, 1023.0),
        ];
        assert_eq!(pressure_axis(&unlabelled_tablet).map(|a| a.number), Some(2));

        let two_axes = [axis(0, None, 0.0, 100.0), axis(1, None, 0.0, 100.0)];
        assert!(pressure_axis(&two_axes).is_none());

        let flat_third = [
            axis(0, None, 0.0, 100.0),
            axis(1, None, 0.0, 100.0),
            axis(2, None, 0.0, 1.0),
        ];
        assert!(pressure_axis(&flat_third).is_none());

        let relative_third = [
            axis(0, None, 0.0, 100.0),
            axis(1, None, 0.0, 100.0),
            axis(2, Some("Rel Wheel"), 0.0, 10.0),
        ];
        assert!(pressure_axis(&relative_third).is_none());
    }

    #[test]
    fn a_pen_event_without_the_pressure_axis_keeps_the_last_value() {
        let mut pen = PenAxis::new(&axis(2, Some("Abs Pressure"), 0.0, 2047.0));
        assert_eq!(pen.last, 0.0);
        assert!((pen.update(&[0b111], &[10.0, 20.0, 1023.5]) - 0.5).abs() < 1e-6);
        assert!((pen.update(&[0b011], &[11.0, 21.0]) - 0.5).abs() < 1e-6);
        assert_eq!(pen.update(&[0b100], &[2047.0]), 1.0);
        assert_eq!(pen.update(&[0b100], &[0.0]), 0.0);
    }

    /// With a live X server and no tablet, the listener connects, lists the
    /// devices without panicking and leaves `pressure()` at `None`.
    #[cfg(all(feature = "window", unix, not(target_os = "macos")))]
    #[test]
    fn x11_listener_runs_without_a_tablet() {
        use std::time::{Duration, Instant};

        if std::env::var("DISPLAY").map_or(true, |display| display.is_empty()) {
            eprintln!("skipped: no DISPLAY");
            return;
        }
        let _turn = turn();
        let mut listener = match platform::x11::Listener::open() {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("skipped: {err}");
                return;
            }
        };
        let devices = listener.describe();
        assert!(
            !devices.is_empty(),
            "an X server always has its core devices"
        );
        for line in &devices {
            eprintln!("{line}");
        }
        expire_pen_hold();
        clear();
        let until = Instant::now() + Duration::from_millis(300);
        while Instant::now() < until {
            match listener.poll() {
                Ok(Some(event)) => listener.handle(event),
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(err) => panic!("listener failed: {err}"),
            }
        }
        assert_eq!(pressure(), None);
    }

    /// Synthetic raw events take the path a real pen's do: only the source
    /// device's copy counts, raw valuators scale by the advertised range, a
    /// missing valuator keeps the last value, a device the server describes
    /// without pressure clears, and one it will not describe says nothing.
    #[cfg(all(feature = "window", unix, not(target_os = "macos")))]
    #[test]
    fn x11_raw_events_decode_through_the_source_device() {
        use x11rb::protocol::xinput::{
            DeviceType, Fp3232, HierarchyEvent, HierarchyInfo, HierarchyMask, RawMotionEvent,
        };
        use x11rb::protocol::Event;

        if std::env::var("DISPLAY").map_or(true, |display| display.is_empty()) {
            eprintln!("skipped: no DISPLAY");
            return;
        }
        let _turn = turn();
        let mut listener = match platform::x11::Listener::open() {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("skipped: {err}");
                return;
            }
        };
        let raw = |deviceid: u16, sourceid: u16, mask: &[u32], values: &[f64]| {
            let fp = |value: &f64| Fp3232 {
                integral: value.trunc() as i32,
                frac: (value.fract() * 4_294_967_296.0) as u32,
            };
            Event::XinputRawMotion(RawMotionEvent {
                deviceid,
                sourceid,
                valuator_mask: mask.to_vec(),
                axisvalues: values.iter().map(fp).collect(),
                axisvalues_raw: values.iter().map(fp).collect(),
                ..Default::default()
            })
        };
        const PEN: u16 = 200;
        listener.pretend_pen(PEN, &axis(2, Some("Abs Pressure"), 0.0, 2047.0));
        clear();
        listener.handle(raw(PEN, PEN, &[0b111], &[10.0, 20.0, 1023.5]));
        assert!((pressure().expect("the pen spoke") - 0.5).abs() < 1e-3);
        // The master pointer's copy of the same event is ignored.
        listener.handle(raw(2, PEN, &[0b111], &[10.0, 20.0, 2047.0]));
        assert!((pressure().expect("the pen spoke") - 0.5).abs() < 1e-3);
        listener.handle(raw(PEN, PEN, &[0b011], &[11.0, 21.0]));
        assert!((pressure().expect("the pen spoke") - 0.5).abs() < 1e-3);
        listener.handle(raw(PEN, PEN, &[0b100], &[0.0]));
        assert_eq!(pressure(), Some(0.0));

        let mouse = listener
            .mouse()
            .expect("an X server has a pointer with axes");
        // A mouse packet in the same moment as the pen keeps the tip force.
        listener.handle(raw(mouse, mouse, &[0b11], &[1.0, 0.0]));
        assert_eq!(pressure(), Some(0.0));
        expire_pen_hold();
        listener.handle(raw(mouse, mouse, &[0b11], &[1.0, 0.0]));
        assert_eq!(pressure(), None);

        // An empty hierarchy is what the Huion spam looks like. The pen stays
        // cached, so the next raw value is read instead of re-queried away.
        report(0.3);
        listener.handle(Event::XinputHierarchy(HierarchyEvent::default()));
        listener.handle(raw(PEN, PEN, &[0b100], &[2047.0]));
        assert_eq!(pressure(), Some(1.0));

        // A slave that left is forgotten. Device 200 is not on this server,
        // so the next raw event cannot replace the pressure we just parked.
        listener.handle(Event::XinputHierarchy(HierarchyEvent {
            infos: vec![HierarchyInfo {
                deviceid: PEN,
                attachment: 2,
                type_: DeviceType::SLAVE_POINTER,
                enabled: false,
                flags: HierarchyMask::SLAVE_REMOVED,
            }],
            ..Default::default()
        }));
        report(0.3);
        listener.handle(raw(PEN, PEN, &[0b100], &[2047.0]));
        assert_eq!(pressure(), Some(0.3));
        expire_pen_hold();
        clear();
    }

    /// The Wayland tool state machine, without a compositor.
    #[cfg(all(feature = "window", unix, not(target_os = "macos")))]
    mod wayland_pens {
        use super::turn;
        use crate::tablet::platform::wayland::{PenEvent, Pens, PROBE_FRAMES};
        use crate::tablet::{clear, expire_pen_hold, pressure, report};

        fn hovering_pen() -> Pens {
            let mut pens = Pens::default();
            pens.added();
            assert!(!pens.tool(0, PenEvent::HasPressure));
            assert!(!pens.tool(0, PenEvent::ProximityIn));
            pens
        }

        #[test]
        fn a_pen_reports_through_frames_and_reads_zero_once_lifted() {
            let _turn = turn();
            clear();
            let mut pens = hovering_pen();
            assert!(!pens.tool(0, PenEvent::Frame));
            assert_eq!(pressure(), Some(0.0));
            // The compositor's emulated pointer follows the hovering pen.
            pens.pointer();
            assert_eq!(pressure(), Some(0.0));
            assert!(!pens.tool(0, PenEvent::Pressure(32768)));
            assert!(!pens.tool(0, PenEvent::Frame));
            assert_eq!(pens.emulation(), Some(true));
            assert!((pressure().expect("a pen spoke") - 0.5).abs() < 1e-3);
            // A frame without pressure keeps the last value.
            assert!(!pens.tool(0, PenEvent::Frame));
            assert!((pressure().expect("a pen spoke") - 0.5).abs() < 1e-3);
            assert!(!pens.tool(0, PenEvent::Up));
            assert!(!pens.tool(0, PenEvent::Frame));
            assert_eq!(pressure(), Some(0.0));
            assert!(!pens.tool(0, PenEvent::ProximityOut));
            assert!(!pens.tool(0, PenEvent::Frame));
            assert_eq!(pressure(), Some(0.0));
            // Out of proximity, a moving pointer is a mouse again once the
            // pen's hold has lapsed.
            expire_pen_hold();
            pens.pointer();
            assert_eq!(pressure(), None);
        }

        #[test]
        fn a_tool_without_pressure_counts_as_a_mouse() {
            let _turn = turn();
            report(0.4);
            let mut pens = Pens::default();
            pens.added();
            assert!(!pens.tool(0, PenEvent::ProximityIn));
            assert!(!pens.tool(0, PenEvent::Pressure(65535)));
            assert!(!pens.tool(0, PenEvent::Frame));
            assert_eq!(
                pressure(),
                Some(0.4),
                "a no-pressure tool in the pen's hold does not drop it"
            );
            expire_pen_hold();
            assert!(!pens.tool(0, PenEvent::Frame));
            assert_eq!(pressure(), None);
        }

        #[test]
        fn a_compositor_without_pointer_emulation_gets_the_seat_back() {
            let _turn = turn();
            clear();
            let mut pens = hovering_pen();
            for _ in 1..PROBE_FRAMES {
                assert!(!pens.tool(0, PenEvent::Pressure(65535)));
                assert!(!pens.tool(0, PenEvent::Frame));
                assert_eq!(pens.emulation(), None);
            }
            assert_eq!(pressure(), Some(1.0));
            expire_pen_hold();
            assert!(
                pens.tool(0, PenEvent::Frame),
                "the last silent frame hands the seat back"
            );
            assert_eq!(pens.emulation(), Some(false));
            assert_eq!(pressure(), None);
            // The tools went with the seat: nothing more is reported.
            assert!(!pens.tool(0, PenEvent::Frame));
            assert_eq!(pressure(), None);
        }

        #[test]
        fn removing_a_hovering_tool_lets_the_mouse_clear_again() {
            let _turn = turn();
            let mut pens = hovering_pen();
            pens.added();
            assert!(!pens.tool(0, PenEvent::Frame));
            assert_eq!(pressure(), Some(0.0));
            pens.removed(0);
            expire_pen_hold();
            pens.pointer();
            assert_eq!(pressure(), None);
            // The second tool slid down to index 0; nothing listens past it.
            assert!(!pens.tool(0, PenEvent::ProximityIn));
            assert!(!pens.tool(1, PenEvent::Frame));
            pens.removed(7);
            assert_eq!(pressure(), None);
        }
    }
}
