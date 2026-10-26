//! Source-local calibration state. Raw values are evdev values, before translation.
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, LazyLock, Mutex, Weak},
};

use evdev::{AbsInfo, AbsoluteAxisCode, InputEvent};
use zbus::{fdo, message::Header, Connection, Proxy};

pub type AxisInfo = (u16, i32, i32, i32, i32, i32, i32);
pub type RawEvent = (u16, u16, i32);
type Registry = HashMap<String, Weak<Mutex<CalibrationState>>>;
static SOURCES: LazyLock<Mutex<Registry>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Default)]
pub struct CalibrationState {
    pub owner: Option<String>,
    pub ready: bool,
    pub refresh: bool,
    pub axes: Vec<AxisInfo>,
    events: VecDeque<RawEvent>,
    overflow: bool,
}

impl CalibrationState {
    pub fn register(path: String) -> Arc<Mutex<Self>> {
        let state = Arc::new(Mutex::new(Self::default()));
        SOURCES.lock().unwrap().insert(path, Arc::downgrade(&state));
        state
    }

    pub fn update_axes(&mut self, axes: &HashMap<AbsoluteAxisCode, AbsInfo>) {
        self.axes = axes
            .iter()
            .map(|(code, info)| {
                (
                    code.0,
                    info.value(),
                    info.minimum(),
                    info.maximum(),
                    info.fuzz(),
                    info.flat(),
                    info.resolution(),
                )
            })
            .collect();
        self.axes.sort_by_key(|axis| axis.0);
        self.refresh = false;
    }

    pub fn record(&mut self, events: &[InputEvent]) {
        for event in events {
            if self.events.len() == 4096 {
                self.overflow = true;
                self.events.pop_front();
            }
            self.events
                .push_back((event.event_type().0, event.code(), event.value()));
        }
    }

    fn release(&mut self) {
        self.owner = None;
        self.ready = false;
        self.events.clear();
    }
}

fn source(path: &str) -> fdo::Result<Arc<Mutex<CalibrationState>>> {
    SOURCES
        .lock()
        .unwrap()
        .get(path)
        .and_then(Weak::upgrade)
        .ok_or_else(|| fdo::Error::NotSupported("No running evdev gamepad source".into()))
}

fn caller(hdr: &Header<'_>) -> fdo::Result<String> {
    hdr.sender()
        .map(|sender| sender.to_string())
        .ok_or_else(|| fdo::Error::AccessDenied("Missing D-Bus sender".into()))
}

fn check_owner(state: &CalibrationState, owner: &str) -> fdo::Result<()> {
    if state.owner.as_deref() != Some(owner) {
        return Err(fdo::Error::AccessDenied(
            "Calibration session belongs to another connection".into(),
        ));
    }
    Ok(())
}

pub async fn begin(path: &str, conn: &Connection, hdr: Header<'_>) -> fdo::Result<()> {
    let owner = caller(&hdr)?;
    let bus = Proxy::new(
        conn,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await?;
    let uid: u32 = bus.call("GetConnectionUnixUser", &(&owner,)).await?;
    if uid != 0 {
        return Err(fdo::Error::AccessDenied(
            "Calibration requires a privileged hardware backend".into(),
        ));
    }
    let shared = source(path)?;
    {
        let mut state = shared.lock().unwrap();
        if state.owner.is_some() {
            return Err(fdo::Error::Failed(
                "Source already has a calibration session".into(),
            ));
        }
        state.owner = Some(owner.clone());
        state.ready = false;
        state.refresh = true;
        state.events.clear();
        state.overflow = false;
    }
    // Poll the bus daemon rather than relying on the client to clean up. A failed
    // liveness check also releases the source so a bus error cannot strand it.
    let conn = conn.clone();
    let weak = Arc::downgrade(&shared);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let Some(shared) = weak.upgrade() else { return };
            if shared.lock().unwrap().owner.as_deref() != Some(&owner) {
                return;
            }
            let alive = async {
                let bus = Proxy::new(
                    &conn,
                    "org.freedesktop.DBus",
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus",
                )
                .await?;
                bus.call::<_, _, bool>("NameHasOwner", &(&owner,)).await
            }
            .await
            .unwrap_or(false);
            if !alive {
                let mut state = shared.lock().unwrap();
                if state.owner.as_deref() == Some(&owner) {
                    state.release();
                }
                return;
            }
        }
    });
    Ok(())
}

pub fn read(
    path: &str,
    hdr: Header<'_>,
) -> fdo::Result<(bool, bool, Vec<AxisInfo>, Vec<RawEvent>)> {
    let shared = source(path)?;
    let mut state = shared.lock().unwrap();
    check_owner(&state, &caller(&hdr)?)?;
    let overflow = std::mem::take(&mut state.overflow);
    Ok((
        state.ready && !state.refresh,
        overflow,
        state.axes.clone(),
        state.events.drain(..).collect(),
    ))
}

pub fn refresh(path: &str, hdr: Header<'_>) -> fdo::Result<()> {
    let shared = source(path)?;
    let mut state = shared.lock().unwrap();
    check_owner(&state, &caller(&hdr)?)?;
    state.refresh = true;
    Ok(())
}

pub fn end(path: &str, hdr: Header<'_>) -> fdo::Result<()> {
    let shared = source(path)?;
    let mut state = shared.lock().unwrap();
    check_owner(&state, &caller(&hdr)?)?;
    state.release();
    Ok(())
}
