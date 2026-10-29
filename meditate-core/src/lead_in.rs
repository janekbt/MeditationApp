//! The Bluetooth wake-up lead-in: a soft noise that wakes sleeping
//! headphones and boxes before a bell. Both apps play these samples.

/// Silence between the noise and the bell: long enough to tell them
/// apart, short enough that a box which mutes its amplifier stays on.
pub const GAP_MS: u32 = 50;

/// The lead-in's length range in ms (Preferences).
pub const MIN_MS: u32 = 100;
pub const MAX_MS: u32 = 5000;

/// After the planned end, a noise whose end never comes lets its bells
/// go: generous, a cold Bluetooth link holds the noise back first.
pub const LATE_MS: u32 = 5000;

/// The rate both apps play the samples at.
pub const RATE: u32 = 48000;

/// The noise's level, of 32768 (about -30 dBFS).
const PEAK: f32 = 1000.0;

/// Fade in and out of the noise, against clicks.
const FADE_MS: u32 = 20;

/// `noise_ms` of white noise at one level as interleaved stereo 16-bit
/// samples at `rate`, faded in and out over `FADE_MS`, then `GAP_MS` of
/// silence; the bell starts where they end. The same length gives the
/// same samples.
pub fn samples(noise_ms: u32, rate: u32) -> Vec<i16> {
    let frames_of = |ms: u32| (u64::from(rate) * u64::from(ms) / 1000) as usize;
    let noise = frames_of(noise_ms);
    let fade = frames_of(FADE_MS).max(1) as f32;
    // A fixed seed: the same every time.
    let mut state = 0x9e37_79b9;
    let mut out = vec![0; (noise + frames_of(GAP_MS)) * 2];
    for (i, sample) in out[..noise * 2].iter_mut().enumerate() {
        let (unit, next) = crate::bells::xorshift64(state);
        state = next;
        let frame = (i / 2) as f32;
        let gain = (frame / fade).min((noise as f32 - 1.0 - frame) / fade).clamp(0.0, 1.0);
        *sample = (PEAK * gain * (unit * 2.0 - 1.0) as f32) as i16;
    }
    out
}

/// `samples` (interleaved stereo) as a 16-bit PCM WAV file at `rate`.
pub fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let data = u32::try_from(samples.len() * 2).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    bytes
}

/// Whether a sound output with these properties is Bluetooth:
/// PipeWire and PulseAudio name the API "bluez5" / "bluez" and the node
/// "bluez_output." / "bluez_sink.".
pub fn is_bluetooth_sink(api: Option<&str>, node: Option<&str>) -> bool {
    api.is_some_and(|a| a.starts_with("bluez")) || node.is_some_and(|n| n.starts_with("bluez_"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lead_in_is_noise_then_a_short_silence() {
        let lead = samples(100, 48000);
        // 100 ms of noise (4800 stereo frames), then 50 ms of silence.
        assert_eq!(lead.len(), (4800 + 2400) * 2);
        let (noise, gap) = lead.split_at(4800 * 2);
        assert!(gap.iter().all(|&s| s == 0));
        assert!(noise.iter().all(|s| s.unsigned_abs() <= 1000));
        // Faded in and out: silent at both ends, at full level between.
        assert_eq!(&noise[..2], &[0, 0]);
        assert!(noise[noise.len() - 2..].iter().all(|s| s.unsigned_abs() <= 10));
        assert!(noise[4000..5600].iter().any(|s| s.unsigned_abs() > 900), "one level in the middle");
        assert!(noise[2000..8000].iter().filter(|&&s| s == 0).count() < 50, "noise, not silence");
        assert_eq!(samples(100, 48000), lead, "the same every time");
        assert_eq!(samples(1000, 44100).len(), (44100 + 2205) * 2);
        // Its WAV holds exactly these samples after a 44-byte header.
        let bytes = wav(&lead, 48000);
        assert_eq!(bytes.len(), 44 + lead.len() * 2);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[4..8], &(36 + lead.len() as u32 * 2).to_le_bytes());
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(&bytes[20..24], &[1, 0, 2, 0], "PCM, stereo");
        assert_eq!(&bytes[24..28], &48000u32.to_le_bytes());
        assert_eq!(&bytes[28..32], &192000u32.to_le_bytes());
        assert_eq!(&bytes[32..36], &[4, 0, 16, 0]);
        assert_eq!(&bytes[36..40], b"data");
        let tiny = wav(&[1, -2], 48000);
        assert_eq!(&tiny[44..], &[1, 0, 0xfe, 0xff]);
    }

    #[test]
    fn a_bluez_sink_counts_as_bluetooth() {
        assert!(is_bluetooth_sink(Some("bluez5"), None));
        assert!(is_bluetooth_sink(Some("bluez"), None));
        assert!(is_bluetooth_sink(None, Some("bluez_output.AA_BB.1")));
        assert!(is_bluetooth_sink(None, Some("bluez_sink.AA_BB.a2dp_sink")));
        assert!(!is_bluetooth_sink(Some("alsa"), Some("alsa_output.pci-0000_00_1f.3.analog-stereo")));
        assert!(!is_bluetooth_sink(None, None));
    }
}
