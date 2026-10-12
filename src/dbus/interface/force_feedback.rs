use std::{error::Error, future::Future};

use packed_struct::types::{Integer, SizedInteger};
use zbus::{fdo, message::Header, Connection};
use zbus_macros::interface;

use crate::{
    dbus::polkit::check_polkit,
    drivers::steam_deck::hid_report::PackedRumbleReport,
    input::{composite_device::client::CompositeDeviceClient, output_event::OutputEvent},
};

use super::Unregisterable;

/// [ForceFeedbacker] is any device that can implement force feedback
pub trait ForceFeedbacker {
    fn get_enabled(&self) -> impl Future<Output = Result<bool, Box<dyn Error>>> + Send;
    fn set_enabled(
        &mut self,
        enabled: bool,
    ) -> impl Future<Output = Result<(), Box<dyn Error>>> + Send;
    fn rumble(&mut self, value: f64) -> impl Future<Output = Result<(), Box<dyn Error>>> + Send;
    fn stop(&mut self) -> impl Future<Output = Result<(), Box<dyn Error>>> + Send;
    fn get_rumble_gain(&self) -> impl Future<Output = Result<f64, Box<dyn Error>>> + Send;
    fn set_rumble_gain(
        &mut self,
        gain: f64,
    ) -> impl Future<Output = Result<(), Box<dyn Error>>> + Send;
}

impl ForceFeedbacker for CompositeDeviceClient {
    async fn get_rumble_gain(&self) -> Result<f64, Box<dyn Error>> {
        Ok(CompositeDeviceClient::get_rumble_gain(self).await?)
    }

    async fn set_rumble_gain(&mut self, gain: f64) -> Result<(), Box<dyn Error>> {
        CompositeDeviceClient::set_rumble_gain(self, gain)
            .await?
            .map_err(|err| err.into())
    }

    async fn get_enabled(&self) -> Result<bool, Box<dyn Error>> {
        Ok(self.get_ff_enabled().await?)
    }

    async fn set_enabled(&mut self, enabled: bool) -> Result<(), Box<dyn Error>> {
        self.set_ff_enabled(enabled).await?;
        Ok(())
    }

    async fn rumble(&mut self, value: f64) -> Result<(), Box<dyn Error>> {
        let value = value.min(1.0);
        let value = value.max(0.0);
        log::debug!("Sending rumble event with value: {value}");
        let report = PackedRumbleReport {
            intensity: (value * u8::MAX as f64) as u8,
            left_speed: Integer::from_primitive((value * u16::MAX as f64) as u16),
            right_speed: Integer::from_primitive((value * u16::MAX as f64) as u16),
            ..Default::default()
        };
        let event = OutputEvent::SteamDeckRumble(report);
        self.process_output_event(event).await?;
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), Box<dyn Error>> {
        self.rumble(0.0).await
    }
}

/// The [ForceFeedbackInterface] provides a DBus interface that can be exposed for
/// managing force feedback events over dbus.
pub struct ForceFeedbackInterface<T>
where
    T: ForceFeedbacker + Send + Sync,
{
    device: T,
}

impl<T> ForceFeedbackInterface<T>
where
    T: ForceFeedbacker + Send + Sync + 'static,
{
    /// Create a new dbus interface for the given force feedback device
    pub fn new(device: T) -> Self {
        Self { device }
    }
}

#[interface(
    name = "org.shadowblip.Output.ForceFeedback",
    proxy(default_service = "org.shadowblip.InputPlumber",)
)]
impl<T> ForceFeedbackInterface<T>
where
    T: ForceFeedbacker + Send + Sync + 'static,
{
    /// Whether or not the device should send force feedback events
    #[zbus(property)]
    async fn enabled(&self) -> fdo::Result<bool> {
        self.device
            .get_enabled()
            .await
            .map_err(|err| fdo::Error::Failed(err.to_string()))
    }
    #[zbus(property)]
    async fn set_enabled(
        &mut self,
        enabled: bool,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Option<Header<'_>>,
    ) -> fdo::Result<()> {
        check_polkit(conn, hdr, "org.shadowblip.Output.ForceFeedback.Enable").await?;
        self.device
            .set_enabled(enabled)
            .await
            .map_err(|err| fdo::Error::Failed(err.to_string()))?;
        Ok(())
    }

    /// Multiplier for rumble amplitudes, in the range 0.0..=2.0 (default 1.0).
    #[zbus(property)]
    async fn rumble_gain(&self) -> fdo::Result<f64> {
        self.device
            .get_rumble_gain()
            .await
            .map_err(|err| fdo::Error::Failed(err.to_string()))
    }

    #[zbus(property)]
    async fn set_rumble_gain(
        &mut self,
        gain: f64,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Option<Header<'_>>,
    ) -> fdo::Result<()> {
        if !gain.is_finite() || !(0.0..=2.0).contains(&gain) {
            return Err(fdo::Error::InvalidArgs(
                "RumbleGain must be finite and between 0.0 and 2.0".into(),
            ));
        }
        check_polkit(conn, hdr, "org.shadowblip.Output.ForceFeedback.Enable").await?;
        self.device
            .set_rumble_gain(gain)
            .await
            .map_err(|err| fdo::Error::Failed(err.to_string()))
    }

    /// Send a simple rumble event
    async fn rumble(
        &mut self,
        value: f64,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        check_polkit(
            conn,
            Some(hdr),
            "org.shadowblip.Output.ForceFeedback.Rumble",
        )
        .await?;
        self.device
            .rumble(value)
            .await
            .map_err(|err| fdo::Error::Failed(err.to_string()))?;
        Ok(())
    }

    /// Stop all currently playing force feedback effects
    async fn stop(
        &mut self,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        check_polkit(conn, Some(hdr), "org.shadowblip.Output.ForceFeedback.Stop").await?;
        self.device
            .stop()
            .await
            .map_err(|err| fdo::Error::Failed(err.to_string()))?;
        Ok(())
    }
}

impl<T> Unregisterable for ForceFeedbackInterface<T> where T: ForceFeedbacker + Send + Sync + 'static
{}
