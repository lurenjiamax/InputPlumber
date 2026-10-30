use std::{collections::HashSet, error::Error, fmt::Debug,
    sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc},
    thread, time::{Duration, Instant}};

use crate::{
    config::ImuConfig,
    drivers::ssc::{self, driver::Driver},
    input::{
        capability::{Capability, Source},
        event::{native::NativeEvent, value::InputValue},
        source::{InputError, SourceInputDevice, SourceOutputDevice},
    },
    udev::device::UdevDevice,
};

pub struct SscImu {
    receiver: mpsc::Receiver<Option<(Instant, Vec<ssc::event::Event>)>>,
    stopping: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    filtered_events: HashSet<Capability>,
}

impl SscImu {
    pub fn new(
        _device_info: UdevDevice,
        imu_config: Option<ImuConfig>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let mount_matrix = imu_config.as_ref().and_then(|c| c.mount_matrix.clone());

        let (sender, receiver) = mpsc::sync_channel(64);
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        // Discovery and synchronous libssc calls must never block composite
        // construction or its input loop. Keep all GLib sensor work here.
        let worker = thread::Builder::new().name("ssc-reconnect".into()).spawn(move || {
            let mut delay = 1;
            while !stop.load(Ordering::Acquire) {
                match Driver::new(mount_matrix.clone()) {
                    Ok(mut driver) => {
                        let connected = Instant::now();
                        log::info!("SSC connected; physical controller remains independent");
                        while !stop.load(Ordering::Acquire) {
                            match driver.poll() {
                                Ok(events) if !events.is_empty() => {
                                    match sender.try_send(Some((Instant::now(), events))) {
                                        Err(mpsc::TrySendError::Disconnected(_)) => return,
                                        _ => (), // Drop excess samples, never stall input.
                                    }
                                    if connected.elapsed() > Duration::from_secs(5) { delay = 1; }
                                }
                                Ok(_) => (),
                                Err(error) => {
                                    log::warn!("SSC disconnected, retrying: {error}");
                                    break;
                                }
                            }
                            thread::park_timeout(Duration::from_millis(2));
                        }
                        // Release old objects before a fresh connection attempt.
                        drop(driver);
                        let _ = sender.try_send(None);
                    }
                    Err(error) => log::warn!("SSC unavailable; retry in {delay}s: {error}"),
                }
                if stop.load(Ordering::Acquire) { break; }
                thread::park_timeout(Duration::from_secs(delay));
                delay = (delay * 2).min(8);
            }
        })?;
        Ok(Self { receiver, stopping, worker: Some(worker), filtered_events: HashSet::new() })
    }
}

impl SourceInputDevice for SscImu {
    /// Poll the given input device for input events
    fn poll(&mut self) -> Result<Vec<NativeEvent>, InputError> {
        let mut accel = None;
        let mut gyro = None;
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                Some((received, events)) if received.elapsed() < Duration::from_millis(250) => {
                    for event in events {
                        match event {
                            ssc::event::Event::Accelerometer(_) => accel = Some(event),
                            ssc::event::Event::Gyro(_) => gyro = Some(event),
                        }
                    }
                }
                _ => { accel = None; gyro = None; }
            }
        }
        let mut latest: Vec<_> = accel.into_iter().chain(gyro).collect();
        latest.retain(|event| {
            let capability = match event {
                ssc::event::Event::Accelerometer(_) => Capability::Accelerometer(Source::Center),
                ssc::event::Event::Gyro(_) => Capability::Gyroscope(Source::Center),
            };
            !self.filtered_events.contains(&capability)
        });
        Ok(translate_events(latest))
    }

    /// Returns the possible input events this device is capable of emitting
    fn get_capabilities(&self) -> Result<Vec<Capability>, InputError> {
        Ok(CAPABILITIES.into())
    }

    fn update_event_filter(&mut self, events: HashSet<Capability>) -> Result<(), InputError> {
        self.filtered_events = events;
        Ok(())
    }

    fn get_default_event_filter(&self) -> Result<HashSet<Capability>, InputError> {
        Ok(HashSet::new())
    }
}

impl SourceOutputDevice for SscImu {}

impl Debug for SscImu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SscImu").finish()
    }
}

impl Drop for SscImu {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            // Cleanup remains on the SSC thread; do not block physical input
            // on a synchronous firmware close operation.
        }
    }
}

/// Translate the given driver events into native events
fn translate_events(events: Vec<ssc::event::Event>) -> Vec<NativeEvent> {
    events.into_iter().map(translate_event).collect()
}

/// Translate the given driver event into a native event
fn translate_event(event: ssc::event::Event) -> NativeEvent {
    match event {
        ssc::event::Event::Accelerometer(data) => {
            let cap = Capability::Accelerometer(Source::Center);
            let value = InputValue::Vector3 {
                x: Some(data.roll),
                y: Some(data.pitch),
                z: Some(data.yaw),
            };
            NativeEvent::new(cap, value)
        }
        ssc::event::Event::Gyro(data) => {
            let cap = Capability::Gyroscope(Source::Center);
            let value = InputValue::Vector3 {
                // Current InputPlumber native gyro units are rad/s. Target drivers
                // perform their own HID encoding; do not apply the PR's legacy x16.
                x: Some(data.roll),
                y: Some(data.pitch),
                z: Some(data.yaw),
            };
            NativeEvent::new(cap, value)
        }
    }
}

/// List of all capabilities that the driver implements
pub const CAPABILITIES: &[Capability] = &[
    Capability::Accelerometer(Source::Center),
    Capability::Gyroscope(Source::Center),
];
