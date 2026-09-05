//! Synthetic protocol 776 Handshake/Status fixtures.
use bytes::Bytes;
use mc_protocol::{CodecError, framing::RawPacket, status, types::encode_string};

#[test]
fn handshake_wire_vector() {
    // protocol 776 = 88 06, address "localhost", port 25565 = 63 dd, intent 1.
    assert_eq!(
        status::handshake("localhost", 25565).unwrap(),
        [0x88, 6, 9, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', 0x63, 0xdd, 1]
    );
    assert_eq!(status::handshake(&"a".repeat(256), 1), Err(CodecError::StringTooLong));
}

#[test]
fn status_checks_packet_id_string_and_trailing_data() {
    let mut body = Vec::new();
    encode_string("{}", status::MAX_JSON_UNITS, &mut body).unwrap();
    let mut packet = RawPacket { id: status::RESPONSE_ID, payload: Bytes::from(body.clone()) };
    assert_eq!(status::response(&packet), Ok("{}"));
    packet.id = 1;
    assert_eq!(status::response(&packet), Err(CodecError::InvalidPacketId(1)));
    packet.id = 0;
    body.push(0);
    packet.payload = Bytes::from(body);
    assert_eq!(status::response(&packet), Err(CodecError::TrailingData));
}

#[test]
fn ping_pong_preserves_signed_payload_and_requires_exact_width() {
    let mut packet =
        RawPacket { id: status::PONG_ID, payload: Bytes::copy_from_slice(&status::ping(-1)) };
    assert_eq!(packet.payload.as_ref(), &[0xff; 8]);
    assert_eq!(status::pong(&packet), Ok(-1));
    packet.payload = Bytes::from_static(&[0; 7]);
    assert_eq!(status::pong(&packet), Err(CodecError::UnexpectedEof));
    packet.payload = Bytes::from_static(&[0; 9]);
    assert_eq!(status::pong(&packet), Err(CodecError::TrailingData));
}
