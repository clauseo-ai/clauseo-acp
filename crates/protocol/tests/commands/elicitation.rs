//! Elicitation answers: values are strict JSON input, whatever format carried them.

use clauseo_protocol::{ElicitationResponse, HostCommand, RawPayload};

use super::fixtures::{elicitation_id, thread_id};
use super::{Validate, invalid};

/// A JSON copy writes any map key as a string, but a command is strict input: values from
/// `MessagePack` with an integer or binary key, at the top or nested, are refused.
#[test]
fn elicitation_values_with_a_key_that_is_not_a_string_are_refused() {
    let one = |key: RawPayload, value| RawPayload::Map(vec![(key, value)].into());
    for key in [RawPayload::U64(7), RawPayload::Bytes(vec![7; 16].into())] {
        let top = one(key, RawPayload::Null);
        let nested = one(
            RawPayload::Str("v".into()),
            RawPayload::Seq(vec![top.clone()].into()),
        );
        for values in [top, nested] {
            let command = HostCommand::AnswerElicitation {
                thread: thread_id(1),
                elicitation: elicitation_id(4),
                response: ElicitationResponse::Accept { values },
            };
            let packed = rmp_serde::to_vec_named(&command).unwrap();
            let read: HostCommand = rmp_serde::from_slice(&packed).unwrap();
            assert!(invalid(&read.validate()), "{read:?}");
        }
    }
}
