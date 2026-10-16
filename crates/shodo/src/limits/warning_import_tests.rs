use super::*;

fn input() -> Vec<Warning> {
    vec![
        Warning {
            kind: WarningKind::NegativeInput,
            message: "first".into(),
        },
        Warning {
            kind: WarningKind::Suppressed,
            message: "source marker".into(),
        },
        Warning {
            kind: WarningKind::Unsupported,
            message: "third".into(),
        },
        Warning {
            kind: WarningKind::NonFiniteInput,
            message: "fourth".into(),
        },
    ]
}

fn reset() {
    BORROWED_WARNING_VISITS.with(|n| n.set(0));
    BORROWED_WARNING_CLONES.with(|n| n.set(0));
}

#[test]
fn zero_and_already_suppressed_imports_skip_payload_clones_and_tail_walks() {
    let warnings: Vec<_> = (0..128).flat_map(|_| input()).collect();
    let mut sink = WarningSink::new(Some(0));
    reset();
    sink.extend_from_slice(&warnings);
    assert_eq!(BORROWED_WARNING_CLONES.with(|n| n.get()), 0);
    assert_eq!(BORROWED_WARNING_VISITS.with(|n| n.get()), 1);
    assert_eq!(
        sink.as_slice(),
        &[Warning {
            kind: WarningKind::Suppressed,
            message: "further warnings suppressed".into()
        }]
    );
    reset();
    sink.extend_from_slice(&warnings);
    assert_eq!(BORROWED_WARNING_CLONES.with(|n| n.get()), 0);
    assert_eq!(BORROWED_WARNING_VISITS.with(|n| n.get()), 0);
    assert!(!sink.checkpoint().unchanged(&sink));
}

#[test]
fn imports_clone_only_the_remaining_prefix_then_add_one_marker() {
    let warnings: Vec<_> = (0..128).flat_map(|_| input()).collect();
    let mut sink = WarningSink::new(Some(3));
    sink.push(WarningKind::Saturated, "preexisting");
    reset();
    sink.extend_from_slice(&warnings);
    assert_eq!(BORROWED_WARNING_CLONES.with(|n| n.get()), 2);
    assert_eq!(BORROWED_WARNING_VISITS.with(|n| n.get()), 3);
    assert_eq!(sink.as_slice()[0].message, "preexisting");
    assert_eq!(sink.as_slice()[1], warnings[0]);
    assert_eq!(sink.as_slice()[2], warnings[1]);
    assert_eq!(sink.as_slice()[3].message, "further warnings suppressed");
    assert!(sink.is_suppressed());
    assert_eq!(sink.remaining_limit(), Some(0));
}

#[test]
fn borrowed_chunks_preserve_eager_order_duplicates_and_sink_state() {
    let warnings = input();
    for cap in [Some(0), Some(1), Some(3), Some(5), Some(1024), None] {
        for prefill in [0, 2, 6] {
            let mut borrowed = WarningSink::new(cap);
            let mut eager = WarningSink::new(cap);
            for _ in 0..prefill {
                borrowed.push(WarningKind::Saturated, "preexisting");
                eager.push(WarningKind::Saturated, "preexisting");
            }
            for chunk in [
                &warnings[..0],
                &warnings[..2],
                &warnings[2..],
                &warnings[..],
            ] {
                let remaining = borrowed.remaining_limit();
                let suppressed = borrowed.is_suppressed();
                reset();
                borrowed.extend_from_slice(chunk);
                let copies = if suppressed {
                    0
                } else {
                    remaining.map_or(chunk.len(), |n| chunk.len().min(n as usize))
                };
                assert_eq!(
                    BORROWED_WARNING_CLONES.with(|n| n.get()),
                    copies,
                    "{cap:?}/{prefill}"
                );
                for w in chunk {
                    eager.push(w.kind, w.message.clone());
                }
                assert_eq!(borrowed.as_slice(), eager.as_slice(), "{cap:?}/{prefill}");
                assert_eq!(borrowed.is_suppressed(), eager.is_suppressed());
                assert_eq!(borrowed.checkpoint(), eager.checkpoint());
                assert_eq!(borrowed.remaining_limit(), eager.remaining_limit());
            }
        }
    }
}

#[test]
fn incoming_suppressed_kind_is_an_ordinary_record_with_room() {
    let warnings = input();
    let mut sink = WarningSink::new(Some(5));
    sink.extend_from_slice(&warnings);
    assert_eq!(sink.as_slice(), warnings);
    assert!(!sink.is_suppressed());
    assert_eq!(sink.as_slice().len(), 4);
    assert!(sink.checkpoint().unchanged(&sink));
    sink.push(WarningKind::NegativeInput, "after source marker");
    assert_eq!(sink.as_slice()[4].message, "after source marker");
    assert_eq!(sink.as_slice().len(), 5);
    assert!(sink.checkpoint().unchanged(&sink));
}

#[test]
fn empty_chunks_take_and_cap_changes_keep_existing_lifecycle() {
    let warnings = input();
    let mut sink = WarningSink::new(Some(0));
    reset();
    sink.extend_from_slice(&[]);
    assert!(!sink.is_suppressed());
    assert_eq!(sink.as_slice().len(), 0);
    assert!(sink.checkpoint().unchanged(&sink));
    sink.extend_from_slice(&warnings);
    sink.set_max(None);
    reset();
    sink.extend_from_slice(&warnings);
    assert_eq!(BORROWED_WARNING_CLONES.with(|n| n.get()), 0);
    assert_eq!(BORROWED_WARNING_VISITS.with(|n| n.get()), 0);
    assert_eq!(sink.take().len(), 1);
    sink.extend_from_slice(&warnings);
    assert_eq!(sink.as_slice(), warnings);
    assert_eq!(BORROWED_WARNING_CLONES.with(|n| n.get()), 4);
    assert_eq!(sink.remaining_limit(), None);
    assert_eq!(sink.as_slice().len(), 4);
    assert!(sink.checkpoint().unchanged(&sink));
}
