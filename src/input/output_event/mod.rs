use std::sync::mpsc::Sender;

use ::evdev::{FFEffectData, FFEffectKind, InputEvent};

use crate::drivers::{
    dualsense::hid_report::SetStatePackedOutputData,
    steam_deck::hid_report::{
        PackedHapticPulseReport, PackedHapticReport, PackedRumbleReport, PadSide,
    },
};

use super::output_capability::{Haptic, OutputCapability};

/// Output events are events that flow from target devices back to source devices
#[derive(Debug, Clone)]
pub enum OutputEvent {
    Evdev(InputEvent),
    Uinput(UinputOutputEvent),
    DualSense(SetStatePackedOutputData),
    SteamDeckHaptics(PackedHapticReport),
    SteamDeckHapticPulse(PackedHapticPulseReport),
    SteamDeckRumble(PackedRumbleReport),
    GenericRumble {
        weak_magnitude: u16,
        strong_magnitude: u16,
    },
}

impl OutputEvent {
    /// Scale rumble amplitudes without changing timing, flags or stop events.
    /// Trackpad haptics, adaptive triggers and non-rumble FF effects are untouched.
    pub fn with_rumble_gain(mut self, gain: f64) -> Self {
        if gain == 1.0 {
            return self;
        }
        let scale = |value: u16| {
            (f64::from(value) * gain)
                .round()
                .clamp(0.0, u16::MAX as f64) as u16
        };
        match &mut self {
            Self::GenericRumble {
                weak_magnitude,
                strong_magnitude,
            } => {
                *weak_magnitude = scale(*weak_magnitude);
                *strong_magnitude = scale(*strong_magnitude);
            }
            Self::SteamDeckRumble(report) => {
                use packed_struct::types::SizedInteger;
                report.left_speed = scale(report.left_speed.to_primitive()).into();
                report.right_speed = scale(report.right_speed.to_primitive()).into();
            }
            Self::DualSense(report) => {
                report.rumble_emulation_left = (f64::from(report.rumble_emulation_left) * gain)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                report.rumble_emulation_right = (f64::from(report.rumble_emulation_right) * gain)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
            _ => {}
        }
        self
    }

    /// Returns the capability of the output event
    pub fn as_capability(&self) -> Vec<OutputCapability> {
        match self {
            OutputEvent::Evdev(event) => match event.destructure() {
                evdev::EventSummary::Synchronization(_, _, _) => {
                    vec![OutputCapability::NotImplemented]
                }
                evdev::EventSummary::Key(_, _, _) => vec![OutputCapability::NotImplemented],
                evdev::EventSummary::RelativeAxis(_, _, _) => {
                    vec![OutputCapability::NotImplemented]
                }
                evdev::EventSummary::AbsoluteAxis(_, _, _) => {
                    vec![OutputCapability::NotImplemented]
                }
                evdev::EventSummary::Misc(_, _, _) => vec![OutputCapability::NotImplemented],
                evdev::EventSummary::Switch(_, _, _) => vec![OutputCapability::NotImplemented],
                evdev::EventSummary::Led(_, _, _) => vec![OutputCapability::NotImplemented],
                evdev::EventSummary::Sound(_, _, _) => vec![OutputCapability::NotImplemented],
                evdev::EventSummary::Repeat(_, _, _) => vec![OutputCapability::NotImplemented],
                evdev::EventSummary::ForceFeedback(_, _, _) => {
                    vec![OutputCapability::ForceFeedback]
                }
                evdev::EventSummary::Power(_, _, _) => vec![OutputCapability::NotImplemented],
                evdev::EventSummary::ForceFeedbackStatus(_, _, _) => {
                    vec![OutputCapability::NotImplemented]
                }
                evdev::EventSummary::UInput(_, _, _) => vec![OutputCapability::NotImplemented],
                evdev::EventSummary::Other(_, _, _) => vec![OutputCapability::NotImplemented],
            },
            OutputEvent::Uinput(uinput) => match uinput {
                UinputOutputEvent::FFUpload(_, _, _) => vec![OutputCapability::ForceFeedbackUpload],
                UinputOutputEvent::FFErase(_) => vec![OutputCapability::ForceFeedbackErase],
            },
            OutputEvent::DualSense(report) => {
                if report.use_rumble_not_haptics {
                    vec![OutputCapability::ForceFeedback]
                } else {
                    vec![OutputCapability::NotImplemented]
                }
            }
            OutputEvent::SteamDeckHaptics(packed_haptic_report) => {
                match packed_haptic_report.side {
                    PadSide::Left => vec![OutputCapability::Haptics(Haptic::TrackpadLeft)],
                    PadSide::Right => vec![OutputCapability::Haptics(Haptic::TrackpadRight)],
                    PadSide::Both => vec![
                        OutputCapability::Haptics(Haptic::TrackpadLeft),
                        OutputCapability::Haptics(Haptic::TrackpadRight),
                    ],
                }
            }
            OutputEvent::SteamDeckHapticPulse(report) => match report.side {
                PadSide::Left => vec![OutputCapability::Haptics(Haptic::TrackpadLeft)],
                PadSide::Right => vec![OutputCapability::Haptics(Haptic::TrackpadRight)],
                PadSide::Both => vec![
                    OutputCapability::Haptics(Haptic::TrackpadLeft),
                    OutputCapability::Haptics(Haptic::TrackpadRight),
                ],
            },
            OutputEvent::SteamDeckRumble(_) => vec![OutputCapability::ForceFeedback],
            OutputEvent::GenericRumble { .. } => vec![OutputCapability::ForceFeedback],
        }
    }

    /// Returns true if the output event is a force feedback/rumble event
    pub fn is_force_feedback(&self) -> bool {
        match self {
            OutputEvent::Evdev(event) => matches!(
                event.destructure(),
                evdev::EventSummary::ForceFeedback(_, _, _)
            ),
            OutputEvent::Uinput(_) => true,
            OutputEvent::DualSense(report) => report.use_rumble_not_haptics,
            OutputEvent::SteamDeckHaptics(_) => true,
            OutputEvent::SteamDeckHapticPulse(_) => true,
            OutputEvent::SteamDeckRumble(_) => true,
            OutputEvent::GenericRumble { .. } => true,
        }
    }
}

/// Scale only rumble effects; preserve the original data for later gain changes.
pub fn with_rumble_gain(mut effect: FFEffectData, gain: f64) -> FFEffectData {
    if gain == 1.0 {
        return effect;
    }
    if let FFEffectKind::Rumble {
        strong_magnitude,
        weak_magnitude,
    } = &mut effect.kind
    {
        let scale = |value: u16| {
            (f64::from(value) * gain)
                .round()
                .clamp(0.0, u16::MAX as f64) as u16
        };
        *strong_magnitude = scale(*strong_magnitude);
        *weak_magnitude = scale(*weak_magnitude);
    }
    effect
}

#[derive(Debug, Clone)]
pub enum UinputOutputEvent {
    /// Effect data to upload to a source device and a channel to send back
    /// the effect ID.
    FFUpload(i16, FFEffectData, Sender<Option<i16>>),
    /// Effect id to erase
    FFErase(u32),
}
