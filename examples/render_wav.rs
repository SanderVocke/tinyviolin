#![allow(clippy::cast_possible_truncation)] // Bounded f32 audio is intentionally quantized to PCM16.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;
use tinyviolin::{Event, Instrument, Synth, TimedEvent, VoiceId};

const SAMPLE_RATE: u32 = 48_000;
const SAMPLE_RATE_HZ: f32 = 48_000.0;
const SECTION_SAMPLES: usize = 48_000;
const NOTE_OFF_SAMPLE: usize = 31_200;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let presets = [
        ("sine", Instrument::Sine, 440.0),
        ("square", Instrument::Square, 330.0),
        ("triangle", Instrument::Triangle, 261.63),
        ("bass", Instrument::Bass, 110.0),
        ("pad", Instrument::Pad, 220.0),
        ("lead", Instrument::Lead, 440.0),
        ("bass drum", Instrument::BassDrum, 60.0),
        ("tom", Instrument::Tom, 130.0),
        ("snare", Instrument::Snare, 180.0),
        ("hi-hat", Instrument::HiHat, 6_000.0),
    ];

    let mut rendered = Vec::with_capacity(presets.len() * SECTION_SAMPLES);
    for (index, (name, instrument, frequency_hz)) in presets.into_iter().enumerate() {
        let mut synth = Synth::<4>::new(SAMPLE_RATE_HZ)?;
        let id = VoiceId(u64::try_from(index)? + 1);
        let note_on = TimedEvent::new(
            2_400,
            Event::NoteOn {
                id,
                instrument,
                frequency_hz,
                gain: 0.65,
            },
        );
        let mut section = vec![0.0_f32; SECTION_SAMPLES];
        if matches!(
            instrument,
            Instrument::BassDrum | Instrument::Tom | Instrument::Snare | Instrument::HiHat
        ) {
            synth.process(&mut section, &[note_on])?;
        } else {
            synth.process(
                &mut section,
                &[
                    note_on,
                    TimedEvent::new(NOTE_OFF_SAMPLE, Event::NoteOff(id)),
                ],
            )?;
        }
        eprintln!("rendered section {}: {name}", index + 1);
        rendered.extend_from_slice(&section);
    }

    let path = Path::new("rendered/tinyviolin_presets.wav");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_pcm16_wav(path, SAMPLE_RATE, &rendered)?;
    println!(
        "wrote {} mono samples to {}",
        rendered.len(),
        path.display()
    );
    Ok(())
}

fn write_pcm16_wav(path: &Path, sample_rate: u32, samples: &[f32]) -> io::Result<()> {
    let sample_count = u32::try_from(samples.len())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let sample_bytes = sample_count
        .checked_mul(2)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "WAV data is too large"))?;
    let riff_size = 36_u32
        .checked_add(sample_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "WAV file is too large"))?;
    let mut writer = BufWriter::new(File::create(path)?);
    writer.write_all(b"RIFF")?;
    writer.write_all(&riff_size.to_le_bytes())?;
    writer.write_all(b"WAVEfmt ")?;
    writer.write_all(&16_u32.to_le_bytes())?;
    writer.write_all(&1_u16.to_le_bytes())?;
    writer.write_all(&1_u16.to_le_bytes())?;
    writer.write_all(&sample_rate.to_le_bytes())?;
    writer.write_all(&(sample_rate * 2).to_le_bytes())?;
    writer.write_all(&2_u16.to_le_bytes())?;
    writer.write_all(&16_u16.to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&sample_bytes.to_le_bytes())?;
    for &sample in samples {
        let pcm = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        writer.write_all(&pcm.to_le_bytes())?;
    }
    writer.flush()
}
