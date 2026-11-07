use std::{fs::File, io::BufWriter};

use anyhow::Context;
use hound::WavSpec;
use shin_core::format::audio::AudioSource;

use crate::AudioCommand;

pub fn audio_command(command: AudioCommand) -> anyhow::Result<()> {
    match command {
        AudioCommand::Decode {
            audio_path,
            output_path,
        } => {
            let audio = std::fs::read(audio_path).context("Reading input file")?;
            let audio = shin_core::format::audio::read_audio(&audio)?;

            let info = audio.info().clone();

            let writer = File::create(output_path).context("Creating output file")?;
            let writer = BufWriter::new(writer);
            let mut writer = hound::WavWriter::new(
                writer,
                WavSpec {
                    channels: info.channel_count,
                    sample_rate: info.sample_rate,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )
            .context("Creating WAV writer")?;

            let mut audio_source = AudioSource::new(audio.decode().context("Creating decoder")?);

            while let Some((left, right)) = audio_source.read_sample() {
                writer.write_sample(left).context("Writing sample")?;
                writer.write_sample(right).context("Writing sample")?;
            }

            writer.finalize().context("Finalizing the WAV file")?;

            Ok(())
        }
        AudioCommand::Remux {
            audio_path,
            output_path,
        } => {
            let audio = std::fs::read(audio_path).context("Reading input file")?;
            let audio = shin_core::format::audio::read_audio(&audio)?;

            std::fs::write(output_path, shin_core::format::audio::to_ogg_opus(audio))
                .context("Writing output file")?;

            Ok(())
        }
    }
}
