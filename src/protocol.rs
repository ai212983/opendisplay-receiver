//! OpenDisplay wire constants, capability data, and direct-write validation.
//!
//! Field layouts mirror `opendisplay_protocol.h` and the stock
//! `protocol/` + `models/config.py` sources (pinned py-opendisplay
//! checkout). Every frame-size gate takes the product's `frame_bytes`
//! explicitly; nothing here assumes a geometry.

use super::geometry::DisplayGeometry;

pub const IC_TYPE_ESP32: u16 = 9;
pub const IC_TYPE_ESP32_S3: u16 = 2;

/// GATT service and characteristic UUID, string form for `#[gatt_service]`.
pub const SERVICE_UUID_STR: &str = "00002446-0000-1000-8000-00805F9B34FB";
/// Same UUID in BLE little-endian advertising order.
pub const SERVICE_UUID_LE: [u8; 16] = [
    0xfb, 0x34, 0x9b, 0x5f, 0x80, 0x00, 0x00, 0x80, 0x00, 0x10, 0x00, 0x00, 0x46, 0x24, 0x00, 0x00,
];
/// BLE company identifier: also the OD manufacturer-data key and the service's
/// 16-bit alias.
pub const COMPANY_ID: u16 = 0x2446;

/// Max image-data bytes per 0x71 chunk on the unencrypted path.
pub const DATA_CHUNK_MAX: usize = 230;
/// Largest compressed staging accepted. Upstream caps its buffered uploads
/// at 50 KB (`MAX_COMPRESSED_SIZE`); a stock client never sends more, so
/// anything larger is rejected rather than staged. Shared by all products:
/// compressed size is bounded by the client's cap, not the panel.
pub const COMPRESSED_STAGING_MAX: usize = 50 * 1024;
/// Largest DEFLATE window a compressed upload may advertise (RFC 1950
/// CINFO + 8). Canonical firmware compiles its inflater with a 9-bit window
/// and rejects wider streams at the zlib header; enforced up front here.
/// Stock clients send 9-bit streams.
pub const MAX_COMPRESSED_WINDOW_BITS: u8 = 9;
/// `transmission_modes` bit for uncompressed direct-write.
pub const TRANSMISSION_MODE_DIRECT_WRITE: u8 = 0x08;
/// `transmission_modes` bit for ZIP compressed direct-write (bit 0x02).
pub const TRANSMISSION_MODE_ZIP: u8 = 0x02;
/// `transmission_modes` bit for PIPE_WRITE sliding-window transfer (0x10).
/// Set only because the full-frame raw+compressed PIPE subset is
/// implemented and tested; partial, encrypted and LAN PIPE uses stay
/// rejected per the canonical namespaces.
pub const TRANSMISSION_MODE_PIPE: u8 = 0x10;
/// Default `transmission_modes`: direct-write plus ZIP plus the full-frame
/// PIPE_WRITE subset. Receivers that accept only uncompressed direct-write
/// can pass [`TRANSMISSION_MODE_DIRECT_WRITE`] via
/// [`build_config_container_with_modes`].
pub const TRANSMISSION_MODES_DEFAULT: u8 =
    TRANSMISSION_MODE_DIRECT_WRITE | TRANSMISSION_MODE_ZIP | TRANSMISSION_MODE_PIPE;
/// Smallest 0x40 response frame this crate emits: the BLE ATT minimum
/// payload (23-byte MTU minus 3-byte header). Limits below this are
/// refused; larger limits are clamped to [`MAX_RESPONSE_BYTES`].
pub const ATT_MIN_PAYLOAD: usize = 20;
/// Max single GATT write accepted: one full PIPE frame (2-byte opcode +
/// 1-byte seq + payload), which also covers the longest legacy chunk.
pub const MAX_WRITE_BYTES: usize = PIPE_MAX_FRAME;
/// Max bytes per 0x40 response notification.
pub const MAX_RESPONSE_BYTES: usize = 100;

// Command opcodes (16-bit big-endian on the wire).
pub const CMD_CONFIG_READ: u16 = 0x0040;
pub const CMD_CONFIG_WRITE: u16 = 0x0041;
pub const CMD_CONFIG_CHUNK: u16 = 0x0042;
pub const CMD_FIRMWARE_VERSION: u16 = 0x0043;
pub const CMD_READ_MSD: u16 = 0x0044;
pub const CMD_DIRECT_WRITE_START: u16 = 0x0070;
pub const CMD_DIRECT_WRITE_DATA: u16 = 0x0071;
pub const CMD_DIRECT_WRITE_END: u16 = 0x0072;
// PIPE_WRITE sliding-window opcodes (16-bit big-endian on the wire).
// Full-frame raw and compressed only; partial, encrypted and LAN uses are
// rejected per the canonical error namespaces.
pub const CMD_PIPE_WRITE_START: u16 = 0x0080;
pub const CMD_PIPE_WRITE_DATA: u16 = 0x0081;
pub const CMD_PIPE_WRITE_END: u16 = 0x0082;

// Response echo bytes (low byte of the opcode) and status bytes.
pub const RESP_ACK: u8 = 0x00;
pub const RESP_NACK: u8 = 0xFF;
pub const ECHO_CONFIG_READ: u8 = 0x40;
pub const ECHO_CONFIG_WRITE: u8 = 0x41;
pub const ECHO_CONFIG_CHUNK: u8 = 0x42;
pub const ECHO_FIRMWARE_VERSION: u8 = 0x43;
pub const ECHO_MSD_READ: u8 = 0x44;
pub const ECHO_DIRECT_WRITE_START: u8 = 0x70;
pub const ECHO_DIRECT_WRITE_DATA: u8 = 0x71;
pub const ECHO_DIRECT_WRITE_END: u8 = 0x72;
// PIPE response echoes (low byte of the opcode).
pub const ECHO_PIPE_WRITE_START: u8 = 0x80;
pub const ECHO_PIPE_WRITE_DATA: u8 = 0x81;
pub const ECHO_PIPE_WRITE_END: u8 = 0x82;
pub const ECHO_REFRESH_SUCCESS: u8 = 0x73;
pub const ECHO_REFRESH_TIMEOUT: u8 = 0x74;

/// Config container version byte.
pub const CONFIG_VERSION: u8 = 1;
/// Fixed container: 3-byte wrapper + system/manufacturer/power/display
/// packets + 2-byte CRC. The single Display packet keeps its wire length;
/// only its width/height fields vary by geometry.
pub const CONFIG_CONTAINER_LEN: usize = 133;
/// 16-byte manufacturer-specific data record.
pub const MSD_LEN: usize = 16;

/// PIPE_WRITE protocol version carried in the 0x80 request/response.
pub const PIPE_VERSION: u8 = 0x01;
/// 0x80 flags bit0: the streamed bytes are zlib-compressed.
pub const PIPE_FLAG_COMPRESSED: u8 = 0x01;
/// 0x80 flags bit1: partial-region refresh. Never accepted; a PARTIAL
/// START is NACKed with `OD_ERR_PIPE_START_PARTIAL_UNSUPPORTED`.
pub const PIPE_FLAG_PARTIAL: u8 = 0x02;
/// Known 0x80 flags bits. Anything else is
/// `OD_ERR_PIPE_START_UNKNOWN_FLAG`.
pub const PIPE_KNOWN_FLAGS: u8 = PIPE_FLAG_COMPRESSED | PIPE_FLAG_PARTIAL;
/// Max on-wire frame size (GATT write ceiling).
pub const PIPE_MAX_FRAME: usize = 244;
/// Plaintext 0x81 header: 2-byte opcode + 1-byte seq.
pub const PIPE_FRAME_OVERHEAD: usize = 3;
/// Max DATA payload bytes per 0x81 frame at the ceiling.
pub const PIPE_DATA_MAX: usize = PIPE_MAX_FRAME - PIPE_FRAME_OVERHEAD;
/// SACK ack_mask width in bits.
pub const PIPE_ACK_MASK_BITS: u32 = 32;
/// Small bounded receiver window that fits the existing 4-packet transport.
/// The START response advertises these device maxima; the client applies
/// the canonical min-rule, so the effective window never exceeds this.
pub const PIPE_DEV_MAX_WINDOW: u8 = 4;
/// Device max ACK cadence, clamped like the window (`N <= W`).
pub const PIPE_DEV_MAX_ACK_EVERY: u8 = 4;
/// Reorder depth: device max window + 1, so `seq % PIPE_REORDER_SLOTS`
/// indexes live seqs collision-free even across the mod-256 wrap.
pub const PIPE_REORDER_SLOTS: usize = 5;
/// Per-slot payload capacity: the largest DATA payload at the frame ceiling.
pub const PIPE_REORDER_SLOT_SIZE: usize = PIPE_DATA_MAX;

// Canonical 0x80 START error namespace (`[0xFF][0x80][err][0x00]`).
// Scoped to the START opcode; byte values intentionally overlap the 0x76
// partial namespace, so they must never cross opcodes.
pub const OD_ERR_PIPE_START_BAD_HEADER: u8 = 0x01;
pub const OD_ERR_PIPE_START_UNKNOWN_FLAG: u8 = 0x02;
pub const OD_ERR_PIPE_START_SIZE_MISMATCH: u8 = 0x03;
// Canonical namespace siblings, unused by construction: every partial START
// is rejected with PARTIAL_UNSUPPORTED before the etag/rect gates.
// Kept so the namespace stays complete and reviewable against the header.
#[allow(dead_code)]
pub const OD_ERR_PIPE_START_ETAG_MISMATCH: u8 = 0x05;
pub const OD_ERR_PIPE_START_PARTIAL_UNSUPPORTED: u8 = 0x06;
#[allow(dead_code)]
pub const OD_ERR_PIPE_START_RECT_INVALID: u8 = 0x07;

// Handler-local 0x81 DATA NACK errors (`[0xFF][0x81][err][hs][mask LE4]`).
// All fatal: after one, further 0x81 frames are discarded until the next
// START or disconnect.
// Unused by construction: compressed bytes stage opaquely and decode at
// END, whose NACK carries no err byte. Kept so the namespace stays whole.
#[allow(dead_code)]
pub const PIPE_DATA_NACK_DECOMPRESS: u8 = 0x02;
pub const PIPE_DATA_NACK_WRITE: u8 = 0x03;
pub const PIPE_DATA_NACK_STATE: u8 = 0x04;

/// `[ACK][echo]`.
pub const fn ack2(echo: u8) -> [u8; 2] {
    [RESP_ACK, echo]
}

/// `[NACK][echo][error][0x00]`, matching upstream direct-write NACKs.
pub const fn nack4(echo: u8, error: u8) -> [u8; 4] {
    [RESP_NACK, echo, error, 0x00]
}

/// CRC-16/CCITT-FALSE over `data`, forcing the leading 2-byte length field
/// to zero first (it is excluded from the CRC).
pub fn config_crc(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for (i, &byte) in data.iter().enumerate() {
        let byte = if i < 2 { 0 } else { byte };
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Board metadata in the standard manufacturer-data packet (type 0x02).
/// Model numbers are scoped to their manufacturer; products own assignments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardIdentity {
    pub manufacturer_id: u16,
    pub board_type: u8,
    pub board_revision: u8,
}

/// Read-only capability container: caller-selected system IC and board, BLE +
/// power (USB) + one display advertising direct-write plus ZIP plus the
/// full-frame PIPE_WRITE subset (no streaming/G5/partial bits). The
/// container length and CRC rule are independent of geometry and system IC.
/// Invalid or unrepresentable geometry returns None.
pub fn build_config_container(
    geometry: DisplayGeometry,
    ic_type: u16,
    board: BoardIdentity,
) -> Option<[u8; CONFIG_CONTAINER_LEN]> {
    build_config_container_with_modes(geometry, ic_type, board, TRANSMISSION_MODES_DEFAULT)
}

/// Read-only capability container with a caller-selected
/// `transmission_modes` byte (e.g. direct-write-only for products that
/// cannot stage compressed or PIPE transfers). The Flex Display `color_scheme`
/// byte tracks `geometry.format` (MONO 0, Grayscale4 5). Otherwise identical
/// to [`build_config_container`].
/// Invalid or unrepresentable geometry returns None.
pub fn build_config_container_with_modes(
    geometry: DisplayGeometry,
    ic_type: u16,
    board: BoardIdentity,
    transmission_modes: u8,
) -> Option<[u8; CONFIG_CONTAINER_LEN]> {
    if !geometry.is_valid() {
        return None;
    }
    let width = geometry.width_px as u16;
    let height = geometry.height_px as u16;
    let mut out = [0u8; CONFIG_CONTAINER_LEN];
    // The embedded wrapper length names the container for readers that
    // parse the inner bytes (the web tester's parseConfigBytes warns on
    // zero). It sits outside the CRC: `config_crc` zeroes these bytes
    // before checksumming, so populating them leaves the CRC unchanged.
    out[0..2].copy_from_slice(&(CONFIG_CONTAINER_LEN as u16).to_le_bytes());
    out[2] = CONFIG_VERSION;
    let mut at = 3;
    // SystemConfig, 22 bytes.
    out[at] = 0;
    out[at + 1] = 0x01;
    out[at + 2..at + 4].copy_from_slice(&ic_type.to_le_bytes());
    out[at + 4] = 0x01;
    out[at + 6] = 0xFF;
    out[at + 22] = 0xFF;
    out[at + 23] = 0xFF;
    at += 24;
    // ManufacturerData, 22 bytes; simple-config indices and reserved bytes
    // stay zero. Board identity does not change display/transfer capabilities.
    out[at] = 0;
    out[at + 1] = 0x02;
    out[at + 2..at + 4].copy_from_slice(&board.manufacturer_id.to_le_bytes());
    out[at + 4] = board.board_type;
    out[at + 5] = board.board_revision;
    at += 24;
    // PowerOption, 30 bytes: USB powered, no sense pins.
    out[at] = 0;
    out[at + 1] = 0x04;
    out[at + 2] = 2;
    out[at + 10] = 0xFF;
    out[at + 11] = 0xFF;
    out[at + 22] = 0xFF;
    out[at + 23] = 0xFF;
    at += 32;
    // DisplayConfig, 46 bytes: e-paper; payload offset 21 is the Flex
    // `color_scheme` byte (MONO 0, Grayscale4 5) and offset 22 the
    // caller-selected `transmission_modes`.
    out[at] = 0;
    out[at + 1] = 0x20;
    out[at + 2] = 0;
    out[at + 3] = 1;
    out[at + 6..at + 8].copy_from_slice(&width.to_le_bytes());
    out[at + 8..at + 10].copy_from_slice(&height.to_le_bytes());
    out[at + 17] = 0xFF;
    out[at + 18] = 0xFF;
    out[at + 19] = 0xFF;
    out[at + 20] = 0xFF;
    out[at + 21] = 0xFF;
    out[at + 22] = 0;
    out[at + 23] = geometry.format.color_scheme();
    out[at + 24] = transmission_modes;
    out[at + 25] = 0xFF;
    at += 48;
    let crc = config_crc(&out[..at]);
    out[at..at + 2].copy_from_slice(&crc.to_le_bytes());
    Some(out)
}

/// One 0x40 response notification for `chunk`, or `None` past the last chunk.
/// Chunk 0 carries `[ACK][0x40][chunk:2 LE][total:2 LE][data]`; later chunks
/// carry `[ACK][0x40][chunk:2 LE][data]`. Frames never exceed 100 bytes.
pub fn config_response_frame(
    container: &[u8; CONFIG_CONTAINER_LEN],
    chunk: u16,
) -> Option<([u8; MAX_RESPONSE_BYTES], usize)> {
    config_response_frame_with_limit(container, chunk, MAX_RESPONSE_BYTES)
}

/// One 0x40 response notification for `chunk`, capped at `max_len` bytes to
/// fit the negotiated ATT payload (`max_len` below [`ATT_MIN_PAYLOAD`] is
/// refused; above [`MAX_RESPONSE_BYTES`] is clamped). Layout matches
/// [`config_response_frame`]; returns `None` past the last chunk.
pub fn config_response_frame_with_limit(
    container: &[u8; CONFIG_CONTAINER_LEN],
    chunk: u16,
    max_len: usize,
) -> Option<([u8; MAX_RESPONSE_BYTES], usize)> {
    if max_len < ATT_MIN_PAYLOAD {
        return None;
    }
    let cap = max_len.min(MAX_RESPONSE_BYTES);
    let first_data = cap - 6;
    let rest_data = cap - 4;
    let (offset, header) = if chunk == 0 {
        (0, 6)
    } else {
        let offset = first_data + (chunk as usize - 1) * rest_data;
        if offset >= CONFIG_CONTAINER_LEN {
            return None;
        }
        (offset, 4)
    };
    let mut frame = [0u8; MAX_RESPONSE_BYTES];
    frame[0] = RESP_ACK;
    frame[1] = ECHO_CONFIG_READ;
    frame[2..4].copy_from_slice(&chunk.to_le_bytes());
    let mut at = 4;
    if chunk == 0 {
        frame[4..6].copy_from_slice(&(CONFIG_CONTAINER_LEN as u16).to_le_bytes());
        at = 6;
    }
    let take = (CONFIG_CONTAINER_LEN - offset).min(cap - header);
    frame[at..at + take].copy_from_slice(&container[offset..offset + take]);
    Some((frame, at + take))
}

/// Fixed 16-byte MSD record: company ID + zeroed v1 dynamic block.
/// Layout per `struct MsdAdvertisement`: company `[0:2]`, dynamic `[2:13]`,
/// temperature `[13]`, battery-low `[14]`, status `[15]`.
pub fn build_msd_record() -> [u8; MSD_LEN] {
    let mut out = [0u8; MSD_LEN];
    out[0..2].copy_from_slice(&COMPANY_ID.to_le_bytes());
    // Temperature byte for 25 C: (25 + 40) * 2.
    out[13] = 130;
    out
}

/// `[ACK][0x43][major][minor][shaLen][sha][patch]`. The tag is
/// product-owned (it names the firmware that answers); it must be ASCII and
/// fit the fixed frame; invalid tags return None.
pub fn build_fw_version(tag: &[u8]) -> Option<([u8; 48], usize)> {
    if tag.is_empty() || tag.len() > 40 || !tag.is_ascii() {
        return None;
    }
    let mut out = [0u8; 48];
    out[0] = RESP_ACK;
    out[1] = ECHO_FIRMWARE_VERSION;
    out[2] = 0;
    out[3] = 1;
    out[4] = tag.len() as u8;
    out[5..5 + tag.len()].copy_from_slice(tag);
    out[5 + tag.len()] = 0;
    let len = 6 + tag.len();
    Some((out, len))
}

/// Split `[cmd:2 BE][payload]`; rejects short writes.
pub fn decode_request(bytes: &[u8]) -> Result<(u16, &[u8]), RequestError> {
    if bytes.len() < 2 {
        return Err(RequestError::TooShort);
    }
    Ok((u16::from_be_bytes([bytes[0], bytes[1]]), &bytes[2..]))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestError {
    TooShort,
}

/// Parsed 0x70 START payload. An empty payload opens an uncompressed
/// transfer; a payload of `[uncompressed_len:4 LE][zlib...]` opens a
/// compressed one, mirroring canonical `handleDirectWriteStart` (which
/// treats any payload of 4+ bytes as compressed and NACKs a size mismatch).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartKind {
    Raw,
    Compressed { uncompressed_len: usize },
}

/// Parse a 0x70 START payload. Compressed lengths must name the product's
/// exact frame; anything else cannot decode to a full panel image.
pub fn parse_start_payload(payload: &[u8], frame_bytes: usize) -> Result<StartKind, StartReject> {
    if payload.is_empty() {
        return Ok(StartKind::Raw);
    }
    if payload.len() < 4 {
        return Err(StartReject::Malformed);
    }
    let len = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
    if len != frame_bytes {
        return Err(StartReject::SizeMismatch);
    }
    Ok(StartKind::Compressed {
        uncompressed_len: len,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartReject {
    /// Shorter than the 4-byte size header.
    Malformed,
    /// Size header names anything but the product's frame.
    SizeMismatch,
}

/// Add `chunk_len` to `received`; rejects overflow past the product's frame.
pub fn validate_data_append(
    received: usize,
    chunk_len: usize,
    frame_bytes: usize,
) -> Result<usize, DataReject> {
    let next = received.saturating_add(chunk_len);
    if next > frame_bytes {
        return Err(DataReject::Overflow);
    }
    if chunk_len == 0 {
        return Err(DataReject::Empty);
    }
    Ok(next)
}

/// Add `chunk_len` to compressed `received`; rejects overflow past the
/// staging bound. The byte-exact frame gate happens at decode, not here.
pub fn validate_compressed_append(received: usize, chunk_len: usize) -> Result<usize, DataReject> {
    let next = received.saturating_add(chunk_len);
    if next > COMPRESSED_STAGING_MAX {
        return Err(DataReject::Overflow);
    }
    if chunk_len == 0 {
        return Err(DataReject::Empty);
    }
    Ok(next)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Fewer than 2 staged bytes, or a zlib header that fails FCHECK, names
    /// a non-DEFLATE method, sets the preset-dictionary bit, or advertises a
    /// DEFLATE window wider than [`MAX_COMPRESSED_WINDOW_BITS`].
    HeaderInvalid,
    /// The stream ends before the frame is complete.
    Incomplete,
    /// The stream is structurally corrupt or its Adler-32 checksum fails.
    Corrupt,
    /// Decoded length is not the frame, or staged bytes trail the stream end.
    LengthMismatch,
}

/// Bounded zlib decode of one staged compressed upload into `out`, which
/// must be exactly `frame_bytes` long. Success means the stream decoded to
/// exactly `out.len()` bytes with every staged byte consumed; anything else
/// (truncated, corrupt, checksum-invalid, over-expanding, trailing) is an
/// error and `out` must not be published.
///
/// The caller owns the reusable [`InflateState`](miniz_oxide::inflate::stream::InflateState);
/// it is reset here, so one state serves every END. Header checks mirror the
/// canonical firmware's 9-bit window build, which `miniz_oxide` alone would
/// not enforce.
pub fn decode_compressed_frame(
    staged: &[u8],
    out: &mut [u8],
    state: &mut miniz_oxide::inflate::stream::InflateState,
    frame_bytes: usize,
) -> Result<(), DecodeError> {
    use miniz_oxide::inflate::stream::FullReset;
    use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

    if out.len() != frame_bytes {
        return Err(DecodeError::LengthMismatch);
    }
    if staged.len() < 2 {
        return Err(DecodeError::HeaderInvalid);
    }
    // RFC 1950 header gate, same shape as upstream `zlib_window_bits`.
    let (cmf, flg) = (staged[0], staged[1]);
    if cmf & 0x0F != 8
        || (u16::from(cmf) << 8 | u16::from(flg)) % 31 != 0
        || flg & 0x20 != 0
        || (cmf >> 4) + 8 > MAX_COMPRESSED_WINDOW_BITS
    {
        return Err(DecodeError::HeaderInvalid);
    }
    state.reset_as(FullReset(DataFormat::Zlib));
    let result = miniz_oxide::inflate::stream::inflate(state, staged, out, MZFlush::Finish);
    match result.status {
        Ok(MZStatus::StreamEnd)
            if result.bytes_written == out.len() && result.bytes_consumed == staged.len() =>
        {
            Ok(())
        }
        Ok(_) => Err(DecodeError::LengthMismatch),
        Err(MZError::Buf) if result.bytes_written == out.len() => Err(DecodeError::LengthMismatch),
        // `Buf` with room to spare means the stream needed input it never
        // got; any other failure is structural corruption or Adler-32
        // mismatch.
        Err(MZError::Buf) => Err(DecodeError::Incomplete),
        Err(_) => Err(DecodeError::Corrupt),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataReject {
    Overflow,
    Empty,
}

/// END payload is `[0x00]` (FULL) or empty, which canonical firmware treats
/// as FULL/auto (`directWriteFinishAndRefresh` refreshes FULL when `len ==
/// 0`, and its own auto-complete path invokes the END handler with `len ==
/// 0`). The pinned web tester sends a bare `0072` on the plain FULL path,
/// so rejecting empty NACKs every such upload 0x02 after all chunks ACK.
/// Refresh mode 1 (FAST/PARTIAL) and any other shape are rejected; FULL only.
pub fn validate_end_payload(payload: &[u8]) -> Result<(), EndReject> {
    match payload {
        [] | [0x00] => Ok(()),
        [_] => Err(EndReject::UnsupportedRefreshMode),
        _ => Err(EndReject::Malformed),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReject {
    UnsupportedRefreshMode,
    Malformed,
}

#[cfg(test)]
mod tests {
    use super::super::geometry::{DisplayGeometry, PixelFormat};
    use super::*;

    const BOARD: BoardIdentity = BoardIdentity {
        manufacturer_id: 0,
        board_type: 0,
        board_revision: 0,
    };
    /// Display packet start: 3-byte wrapper + 24-byte system + 24-byte
    /// manufacturer + 32-byte power packets.
    const DISPLAY_AT: usize = 3 + 24 + 24 + 32;

    fn crc_ok(container: &[u8; CONFIG_CONTAINER_LEN]) -> bool {
        let body = CONFIG_CONTAINER_LEN - 2;
        let expect = u16::from_le_bytes([container[body], container[body + 1]]);
        config_crc(&container[..body]) == expect
    }

    #[test]
    fn default_container_fields_and_crc() {
        let geometry = DisplayGeometry::new(600, 600);
        let c = build_config_container(geometry, IC_TYPE_ESP32_S3, BOARD).unwrap();
        assert_eq!(c.len(), CONFIG_CONTAINER_LEN);
        assert_eq!(
            u16::from_le_bytes([c[0], c[1]]),
            CONFIG_CONTAINER_LEN as u16
        );
        assert_eq!(c[2], CONFIG_VERSION);
        assert_eq!(c[DISPLAY_AT], 0);
        assert_eq!(c[DISPLAY_AT + 1], 0x20);
        // Display payload: width/height LE, color_scheme MONO, default modes.
        let base = DISPLAY_AT + 2;
        assert_eq!(u16::from_le_bytes([c[base + 4], c[base + 5]]), 600);
        assert_eq!(u16::from_le_bytes([c[base + 6], c[base + 7]]), 600);
        assert_eq!(c[base + 21], PixelFormat::Monochrome.color_scheme());
        assert_eq!(c[base + 21], 0);
        assert_eq!(c[base + 22], TRANSMISSION_MODES_DEFAULT);
        assert_eq!(c[base + 22], 0x1A);
        assert!(crc_ok(&c));
    }

    #[test]
    fn raw_only_modes_and_gray_scheme() {
        let mono = DisplayGeometry::with_format(480, 800, PixelFormat::Monochrome);
        let raw = build_config_container_with_modes(
            mono,
            IC_TYPE_ESP32_S3,
            BOARD,
            TRANSMISSION_MODE_DIRECT_WRITE,
        )
        .unwrap();
        let base = DISPLAY_AT + 2;
        assert_eq!(raw[base + 22], 0x08);
        assert_eq!(raw[base + 21], 0);
        assert!(crc_ok(&raw));

        // Same geometry through the default builder differs only in the
        // transmission byte (plus the CRC covering it).
        let full = build_config_container(mono, IC_TYPE_ESP32_S3, BOARD).unwrap();
        assert_eq!(full[base + 22], TRANSMISSION_MODES_DEFAULT);
        for (i, (&a, &b)) in raw.iter().zip(full.iter()).enumerate() {
            let in_crc = i >= CONFIG_CONTAINER_LEN - 2;
            if i == DISPLAY_AT + 2 + 22 || in_crc {
                continue;
            }
            assert_eq!(a, b, "byte {i} differs");
        }

        // Grayscale4 advertises color_scheme 5 at the same length/CRC rule.
        let gray = DisplayGeometry::with_format(480, 800, PixelFormat::Grayscale4);
        let g = build_config_container(gray, IC_TYPE_ESP32_S3, BOARD).unwrap();
        assert_eq!(g.len(), CONFIG_CONTAINER_LEN);
        assert_eq!(g[base + 21], 5);
        assert_eq!(g[base + 22], TRANSMISSION_MODES_DEFAULT);
        assert_eq!(u16::from_le_bytes([g[base + 4], g[base + 5]]), 480);
        assert_eq!(u16::from_le_bytes([g[base + 6], g[base + 7]]), 800);
        assert!(crc_ok(&g));
    }

    #[test]
    fn invalid_geometry_builds_nothing() {
        let bad = DisplayGeometry::new(601, 600);
        assert!(build_config_container(bad, IC_TYPE_ESP32_S3, BOARD).is_none());
        assert!(build_config_container_with_modes(bad, IC_TYPE_ESP32_S3, BOARD, 0x08).is_none());
    }

    /// Drain every chunk at `max_len` and rebuild the container body.
    fn reassemble(
        container: &[u8; CONFIG_CONTAINER_LEN],
        max_len: usize,
    ) -> Option<([u8; CONFIG_CONTAINER_LEN], usize)> {
        let mut out = [0u8; CONFIG_CONTAINER_LEN];
        let mut pos = 0usize;
        let mut chunk: u16 = 0;
        loop {
            let (frame, len) = config_response_frame_with_limit(container, chunk, max_len)?;
            assert!(len <= max_len, "chunk {chunk} exceeds limit");
            if chunk == 0 {
                assert_eq!(&frame[4..6], &(CONFIG_CONTAINER_LEN as u16).to_le_bytes());
            }
            let data = if chunk == 0 {
                &frame[6..len]
            } else {
                &frame[4..len]
            };
            assert!(!data.is_empty(), "chunk {chunk} carries no data");
            if pos + data.len() > CONFIG_CONTAINER_LEN {
                return None;
            }
            out[pos..pos + data.len()].copy_from_slice(data);
            pos += data.len();
            chunk += 1;
            if chunk > 32 {
                panic!("chunk walk did not terminate");
            }
            if config_response_frame_with_limit(container, chunk, max_len).is_none() {
                break;
            }
        }
        Some((out, chunk as usize))
    }

    #[test]
    fn wrapper_matches_limit_at_default() {
        let c =
            build_config_container(DisplayGeometry::new(400, 300), IC_TYPE_ESP32, BOARD).unwrap();
        for chunk in 0..4u16 {
            assert_eq!(
                config_response_frame(&c, chunk),
                config_response_frame_with_limit(&c, chunk, MAX_RESPONSE_BYTES)
            );
        }
        assert_eq!(config_response_frame(&c, 4), None);
    }

    #[test]
    fn reassembly_at_default_and_att_minimum() {
        let c =
            build_config_container(DisplayGeometry::new(400, 300), IC_TYPE_ESP32, BOARD).unwrap();
        let (full, full_chunks) = reassemble(&c, MAX_RESPONSE_BYTES).unwrap();
        assert_eq!(full, c);
        assert_eq!(full_chunks, 2);
        // ATT minimum payload (20 B): 14 B in chunk 0, 16 B per later chunk.
        let (min, min_chunks) = reassemble(&c, ATT_MIN_PAYLOAD).unwrap();
        assert_eq!(min, c);
        assert_eq!(min_chunks, 9);
        assert!(min_chunks > full_chunks);
    }

    #[test]
    fn limits_below_att_minimum_refused() {
        let c =
            build_config_container(DisplayGeometry::new(400, 300), IC_TYPE_ESP32, BOARD).unwrap();
        assert!(config_response_frame_with_limit(&c, 0, ATT_MIN_PAYLOAD - 1).is_none());
        assert!(config_response_frame_with_limit(&c, 0, 0).is_none());
        // Oversize limits clamp to the 100-byte default framing.
        assert_eq!(
            config_response_frame_with_limit(&c, 0, MAX_RESPONSE_BYTES + 50),
            config_response_frame(&c, 0)
        );
    }
}
