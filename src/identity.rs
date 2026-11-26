//! Stable radio identity: the `OD…` advertised name and random-static address.
//!
//! The address keeps the chip MAC's device-unique bytes with random-static
//! top bits, so rediscovery finds the same address every boot; the name
//! carries the `OD` prefix the stock client requires. The product derives
//! both once per boot from its own MAC and shows the name in the app UI.

/// `OD` + 6 uppercase hex chars from the last 3 address bytes.
pub fn advertised_name(address: &[u8; 6]) -> [u8; 8] {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = [0u8; 8];
    out[0] = b'O';
    out[1] = b'D';
    for (i, &byte) in address[3..6].iter().enumerate() {
        out[2 + i * 2] = HEX[(byte >> 4) as usize];
        out[3 + i * 2] = HEX[(byte & 0x0F) as usize];
    }
    out
}

/// Stable random-static address from the chip MAC: top bits forced to `11`,
/// the rest device-unique.
pub fn stable_address(mac: &[u8; 6]) -> [u8; 6] {
    let mut out = *mac;
    out[0] = (out[0] & 0x3F) | 0xC0;
    out
}
