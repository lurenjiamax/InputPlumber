use std::collections::HashSet;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::{sync::mpsc, sync::mpsc::error::TryRecvError};

use crate::config::MountMatrix;
use crate::input::capability::{Capability, Source};

use super::event::{AxisData, Event};
use super::runtime::{SscObject, SscRuntime};

static ACTIVE: AtomicBool = AtomicBool::new(false);
struct SensorClaim;
impl Drop for SensorClaim {
    fn drop(&mut self) { ACTIVE.store(false, Ordering::Release); }
}

pub struct Driver {
    runtime: Arc<SscRuntime>,
    _gyroscope: SscObject,
    _accelerometer: SscObject,

    // Event handling / polling
    rx: mpsc::Receiver<Event>,
    filtered_events: HashSet<Capability>,

    mount_matrix: MountMatrix,
    started: Instant,
    last_accel: Option<Instant>,
    last_gyro: Option<Instant>,
    // Last field: release only after sensor objects are closed/unreferenced.
    _claim: SensorClaim,
}

impl Driver {
    pub fn new(mount_matrix: Option<MountMatrix>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        if ACTIVE.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err("Only one SSC source is supported per InputPlumber process".into());
        }
        let claim = SensorClaim;
        let runtime = SscRuntime::load()?;
        let (tx, rx) = mpsc::channel::<Event>(1024);

        let gyroscope = runtime.clone().create_gyroscope()?;
        let gyroscope_tx = tx.clone();
        gyroscope.set_measurement_handler(move |x, y, z| {
            _ = gyroscope_tx.try_send(Event::Gyro(AxisData {
                roll: x as f64,
                pitch: y as f64,
                yaw: z as f64,
            }));
        });

        let accelerometer = runtime.clone().create_accelerometer()?;
        let accelerometer_tx = tx.clone();
        accelerometer.set_measurement_handler(move |x, y, z| {
            _ = accelerometer_tx.try_send(Event::Accelerometer(AxisData {
                roll: x as f64,
                pitch: y as f64,
                yaw: z as f64,
            }));
        });

        Ok(Self {
            runtime,
            _gyroscope: gyroscope,
            _accelerometer: accelerometer,
            rx,
            filtered_events: Default::default(),
            mount_matrix: mount_matrix.unwrap_or_default(),
            started: Instant::now(),
            last_accel: None,
            last_gyro: None,
            _claim: claim,
        })
    }

    pub fn update_filtered_events(&mut self, events: HashSet<Capability>) {
        self.filtered_events = events;
    }

    pub fn get_default_event_filter(
        &self,
    ) -> Result<HashSet<Capability>, Box<dyn Error + Send + Sync>> {
        Ok(HashSet::new())
    }

    /// Poll the device for data
    pub fn poll(&mut self) -> Result<Vec<Event>, Box<dyn Error + Send + Sync>> {
        // libssc uses GLib and relies on the GLib main loop
        // Instead of creating a main loop and main context then managing it alongside InputPlumber's loop etc,
        // we can just perform an iteration of the GLib context every poll
        self.runtime.iterate_glib_main_loop();

        let mut events: Vec<Event> = vec![];

        loop {
            match self.rx.try_recv() {
                Ok(Event::Gyro(mut event)) => {
                    if !event.roll.is_finite() || !event.pitch.is_finite() || !event.yaw.is_finite()
                    {
                        continue;
                    }
                    self.last_gyro = Some(Instant::now());
                    if self
                        .filtered_events
                        .contains(&Capability::Gyroscope(Source::Center))
                    {
                        continue;
                    }

                    self.rotate_value(&mut event);

                    events.push(Event::Gyro(event));
                }

                Ok(Event::Accelerometer(mut event)) => {
                    if !event.roll.is_finite() || !event.pitch.is_finite() || !event.yaw.is_finite()
                    {
                        continue;
                    }
                    self.last_accel = Some(Instant::now());
                    if self
                        .filtered_events
                        .contains(&Capability::Accelerometer(Source::Center))
                    {
                        continue;
                    }

                    self.rotate_value(&mut event);

                    events.push(Event::Accelerometer(event));
                }

                Err(TryRecvError::Empty) => break,
                Err(error) => return Err(format!("Error when handling SSC events: {error}").into()),
            }
        }

        let stale = |time: Option<Instant>| {
            time.map(|t| t.elapsed() > Duration::from_millis(250))
                .unwrap_or(true)
        };
        if self.started.elapsed() > Duration::from_secs(5)
            && (stale(self.last_accel) || stale(self.last_gyro))
        {
            return Err("SSC accel/gyro stream stale; closing motion source (target may retain cached sample)".into());
        }
        Ok(events)
    }

    /// Rotate the given axis data according to the mount matrix. This is used
    /// to calculate the final value according to the sensor oritentation.
    /// This is taken from iio_imu/driver.rs
    // Values are intended to be multiplied as:
    //   x' = mxx * x + myx * y + mzx * z
    //   y' = mxy * x + myy * y + mzy * z
    //   z' = mxz * x + myz * y + mzz * z
    fn rotate_value(&self, value: &mut AxisData) {
        let x = value.roll;
        let y = value.pitch;
        let z = value.yaw;
        let mxx = self.mount_matrix.x[0];
        let myx = self.mount_matrix.x[1];
        let mzx = self.mount_matrix.x[2];
        let mxy = self.mount_matrix.y[0];
        let myy = self.mount_matrix.y[1];
        let mzy = self.mount_matrix.y[2];
        let mxz = self.mount_matrix.z[0];
        let myz = self.mount_matrix.z[1];
        let mzz = self.mount_matrix.z[2];
        value.roll = mxx * x + myx * y + mzx * z;
        value.pitch = mxy * x + myy * y + mzy * z;
        value.yaw = mxz * x + myz * y + mzz * z;
    }
}
