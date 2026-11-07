//! An NXA file's Opus frames as an Ogg Opus stream, losslessly (no re-encoding): what `sdu audio remux` writes and
//! what the launcher's music player hands the web view.

use std::{borrow::Cow, io::Cursor};

use binrw::BinWrite;
use ogg::{PacketWriteEndInfo, writing::PacketWriter};

use super::{AudioFile, AudioInfo};

#[derive(BinWrite)]
#[brw(magic(b"OpusHead"))]
struct OpusIdHeader {
    version: u8,
    channel_count: u8,
    pre_skip: u16,
    input_sample_rate: u32,
    output_gain: u16,
    mapping_family: u8,
}

const SERIAL: u32 = 42;

fn id_header(info: &AudioInfo) -> Vec<u8> {
    assert!(matches!(info.channel_count, 1 | 2));
    let header = OpusIdHeader {
        version: 1,
        channel_count: info.channel_count as u8,
        pre_skip: info.pre_skip,
        input_sample_rate: info.sample_rate,
        output_gain: 0,
        mapping_family: 0,
    };
    let mut bytes = [0; 19];
    header
        .write_le(&mut Cursor::new(bytes.as_mut_slice()))
        .unwrap();
    bytes.to_vec()
}

/// The whole file as Ogg Opus
pub fn to_ogg_opus(audio: AudioFile) -> Vec<u8> {
    let info = audio.info().clone();
    let mut out = Vec::new();
    {
        let mut writer = PacketWriter::new(&mut out);
        let io = "writing to a Vec cannot fail";
        writer
            .write_packet(id_header(&info), SERIAL, PacketWriteEndInfo::EndPage, 0)
            .expect(io);
        // vendor string length + vendor string + user comment count (always 0)
        let comment: &[u8] = b"OpusTags\x11\x00\x00\x00sdu remuxer\x00\x00\x00\x00";
        writer
            .write_packet(Cow::Borrowed(comment), SERIAL, PacketWriteEndInfo::EndPage, 0)
            .expect(io);

        let mut frames = audio.read_frames();
        let mut index = 0u64;
        while let Some(frame) = frames.get_next_frame().map(Vec::from) {
            index += 1;
            let end = if frames.has_next_frame() {
                PacketWriteEndInfo::NormalPacket
            } else {
                PacketWriteEndInfo::EndPage
            };
            writer
                .write_packet(
                    frame,
                    SERIAL,
                    end,
                    index * info.frame_samples as u64,
                )
                .expect(io);
        }
    }
    out
}
