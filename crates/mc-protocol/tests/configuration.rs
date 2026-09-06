//! Configuration client-information vectors for protocol 776.

use mc_protocol::{CodecError, configuration, types::Reader};

#[test]
fn client_information_encodes_the_selected_render_distance() {
    let body = configuration::information(8).unwrap();
    let mut reader = Reader::new(&body);
    assert_eq!(reader.string(16).unwrap(), "en_US");
    assert_eq!(reader.varint().unwrap(), 8);
}

#[test]
fn client_information_rejects_unbounded_render_distances() {
    assert_eq!(configuration::information(1), Err(CodecError::InvalidValue("render distance")));
    assert_eq!(configuration::information(9), Err(CodecError::InvalidValue("render distance")));
}
