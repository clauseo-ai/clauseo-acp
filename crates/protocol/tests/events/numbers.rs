//! Numbers: a `Count` stays at most 2^53 - 1 and a `Finite` refuses NaN and the infinities, in
//! their constructors and on decode from JSON and `MessagePack`; an optional `Count` an older
//! event lacks decodes as `None`.

use std::fmt::Debug;

use clauseo_protocol::{
    BlobHash, BlobRange, ChangeKind, ContentBlock, ContentId, ContextUsage, Cost, Count,
    DomainEvent, FileChange, Finite, HostQuery, ImportSummary, LimitMeter, LimitWindow,
    LimitWindowView, MAX_SAFE, MeterTone, ReadLen, TurnUsage, UsageUpdated, WindowFreshness,
    WindowKind,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::fixtures::{count, decodes_in_both, err_contains, finite, path, round_trips};

/// Checks a type with a `Count` field that `make` fills: 2^53 - 1 round-trips in JSON and
/// `MessagePack`, and the same value with 2^53 or `u64::MAX` in its place fails to decode from
/// JSON and from its `MessagePack` form, with an error naming `Count` and the value.
fn capped<T, F>(make: F)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
    F: Fn(Count) -> T,
{
    let max = make(count(MAX_SAFE));
    round_trips(&max);
    let json = serde_json::to_string(&max).unwrap();
    let max_text = MAX_SAFE.to_string();
    assert!(json.contains(&max_text), "{json}");
    // MessagePack writes the count as a 64-bit integer: a 0xcf marker and 8 bytes, which the
    // over-limit values overwrite in place, so the other fields stay valid.
    let packed = rmp_serde::to_vec_named(&max).unwrap();
    let mut max_form = vec![0xcf];
    max_form.extend(MAX_SAFE.to_be_bytes());
    let at = packed.windows(9).position(|w| w == max_form).unwrap();
    for over in [MAX_SAFE + 1, u64::MAX] {
        let over_json = json.replace(&max_text, &over.to_string());
        let message = format!("Count: {over} is outside");
        err_contains(serde_json::from_str::<T>(&over_json), &message);
        let mut bytes = packed.clone();
        bytes[at + 1..at + 9].copy_from_slice(&over.to_be_bytes());
        err_contains(rmp_serde::from_slice::<T>(&bytes), &message);
    }
}

#[test]
fn count_fields_refuse_values_above_max_safe() {
    capped(|n| UsageUpdated {
        context_used: Some(n),
        context_size: None,
        cost: None,
    });
    capped(|n| UsageUpdated {
        context_used: None,
        context_size: Some(n),
        cost: None,
    });
    capped(|n| TurnUsage {
        total: n,
        ..TurnUsage::default()
    });
    capped(|n| TurnUsage {
        cached_write: Some(n),
        ..TurnUsage::default()
    });
    capped(|n| {
        let after = Some(ContentId::new("ab12").unwrap());
        FileChange::new(path("a.docx"), ChangeKind::Added, None, after, n).unwrap()
    });
    capped(|n| ImportSummary {
        files: 1,
        bytes: n,
        cloud_only: 0,
    });
    capped(|n| ContentBlock::ResourceLink {
        uri: "file:///a.pdf".to_owned(),
        name: "a.pdf".to_owned(),
        mime_type: None,
        title: None,
        description: None,
        size: Some(n),
    });
    capped(|n| BlobRange {
        offset: Count::ZERO,
        end: n,
        text: String::new(),
    });
    capped(|n| HostQuery::ReadBlob {
        hash: BlobHash::from_bytes([7; 32]),
        offset: n,
        len: ReadLen::new(1).unwrap(),
    });
    capped(|n| ContextUsage {
        context_used: Some(n),
        ..ContextUsage::default()
    });

    // `None` is still left out, and the plain form is a plain number.
    let none = UsageUpdated {
        context_used: None,
        context_size: Some(count(5)),
        cost: None,
    };
    assert_eq!(
        serde_json::to_string(&none).unwrap(),
        r#"{"context_size":5}"#
    );
}

#[test]
fn older_events_without_an_optional_count_still_decode() {
    let empty = UsageUpdated {
        context_used: None,
        context_size: None,
        cost: None,
    };
    decodes_in_both(r#"{"usage_updated":{}}"#, &DomainEvent::from(empty));
    let usage = TurnUsage {
        total: count(3),
        input: count(2),
        output: count(1),
        ..TurnUsage::default()
    };
    decodes_in_both(r#"{"total":3,"input":2,"output":1}"#, &usage);
    let link = ContentBlock::ResourceLink {
        uri: "file:///a.pdf".to_owned(),
        name: "a.pdf".to_owned(),
        mime_type: None,
        title: None,
        description: None,
        size: None,
    };
    decodes_in_both(
        r#"{"resource_link":{"uri":"file:///a.pdf","name":"a.pdf"}}"#,
        &link,
    );
}

#[test]
fn counts_clamp_what_an_agent_reports() {
    assert_eq!(Count::clamped(0), count(0));
    assert_eq!(Count::clamped(MAX_SAFE), count(MAX_SAFE));
    assert_eq!(Count::clamped(MAX_SAFE + 1), count(MAX_SAFE));
    assert_eq!(Count::clamped(7), count(7));
    assert_eq!(Count::clamped(u64::MAX), count(MAX_SAFE));
    assert_eq!(Count::from(u32::MAX).get(), u64::from(u32::MAX));
    assert_eq!(Count::default(), Count::ZERO);
    err_contains(Count::new(MAX_SAFE + 1), "is outside 0..=9007199254740991");
    err_contains(count(MAX_SAFE).next(), "is outside 0..=9007199254740991");
}

/// The finite value each test field holds, and the values it must refuse.
const SAMPLE: f64 = 0.4375;
const NOT_FINITE: [f64; 3] = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];

/// The `MessagePack` float64 of `value`: marker 0xcb, then the big-endian bits.
fn float64(value: f64) -> Vec<u8> {
    let mut bytes = vec![0xcb];
    bytes.extend_from_slice(&value.to_bits().to_be_bytes());
    bytes
}

/// Checks a type with a `Finite` field that `make` fills: [`SAMPLE`] round-trips in both
/// formats, and the same `MessagePack` with a NaN or an infinity in its place fails to decode
/// with an error naming the rule.
fn finite_only<T, F>(make: F)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
    F: Fn(Finite) -> T,
{
    let value = make(finite(SAMPLE));
    round_trips(&value);
    let packed = rmp_serde::to_vec_named(&value).unwrap();
    let sample = float64(SAMPLE);
    let at = packed
        .windows(sample.len())
        .position(|window| window == sample.as_slice())
        .unwrap_or_else(|| panic!("no float64 {SAMPLE} in {packed:x?}"));
    for bad in NOT_FINITE {
        let mut corrupt = packed.clone();
        corrupt.splice(at..at + sample.len(), float64(bad));
        let error = rmp_serde::from_slice::<T>(&corrupt)
            .unwrap_err()
            .to_string();
        assert!(error.contains("finite"), "{bad}: {error}");
    }
}

#[test]
fn finite_refuses_nan_and_infinities() {
    for bad in NOT_FINITE {
        assert_eq!(Finite::new(bad), None, "{bad}");
        let error = Finite::try_from(bad).unwrap_err().to_string();
        assert!(error.contains("finite"), "{error}");
    }
    assert_eq!(Finite::new(-0.0).map(Finite::get), Some(-0.0));
    assert_eq!(Finite::try_from(f64::MAX).unwrap().get(), f64::MAX);
    // A plain number in JSON, an integer included; `null` (a non-finite f64's JSON) is refused.
    assert_eq!(serde_json::to_string(&finite(82.5)).unwrap(), "82.5");
    assert_eq!(serde_json::from_str::<Finite>("3").unwrap(), finite(3.0));
    err_contains(
        serde_json::from_str::<Finite>("null"),
        "invalid type: null, expected f64",
    );
}

#[test]
fn finite_fields_refuse_nan_and_infinities() {
    finite_only(|amount| {
        DomainEvent::from(UsageUpdated {
            context_used: None,
            context_size: None,
            cost: Some(Cost {
                amount,
                currency: "USD".to_owned(),
            }),
        })
    });
    finite_only(|used_percent| LimitWindow {
        id: "five_hour".to_owned(),
        kind: WindowKind::Session,
        label: "5-hour".to_owned(),
        used_percent,
        resets_at: None,
        duration_mins: None,
        warning: false,
    });
    finite_only(|used_percent| LimitMeter {
        tone: MeterTone::Near,
        used_percent,
        window_label: "5-hour".to_owned(),
        resets_at: None,
    });
    finite_only(|used_percent| LimitWindowView {
        id: "five_hour".to_owned(),
        kind: WindowKind::Session,
        label: "5-hour".to_owned(),
        used_percent: Some(used_percent),
        tone: MeterTone::Neutral,
        resets_at: None,
        freshness: WindowFreshness::Fresh,
    });
}

#[test]
fn finite_is_totally_ordered_by_total_cmp() {
    let values = [3.0, f64::MAX, -0.0, 0.0, -f64::MAX, -2.5];
    let mut sorted: Vec<Finite> = values.iter().map(|&v| finite(v)).collect();
    sorted.sort();
    let expected = [-f64::MAX, -2.5, -0.0, 0.0, 3.0, f64::MAX];
    assert_eq!(sorted.iter().map(|f| f.get()).collect::<Vec<_>>(), expected);
    // Equality agrees with the order, so `-0.0` and `0.0` are two values, as their JSON is.
    assert_eq!(finite(1.5), finite(1.5));
    assert_ne!(finite(-0.0), finite(0.0));
    assert_eq!(finite(-0.0).cmp(&finite(0.0)), std::cmp::Ordering::Less);
    assert_eq!(
        finite(1.0).partial_cmp(&finite(2.0)),
        Some(std::cmp::Ordering::Less)
    );
}
