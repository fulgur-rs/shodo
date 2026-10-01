//! Interning keeps Debug equality, but preflights keys without allocating them.
use crate::limits::{LimitExceeded, LimitKind, Limits};
use crate::style::InlineStyle;
use std::fmt::{self, Write};
use std::hash::{BuildHasher, Hasher};

type Pair<'a> = (&'a InlineStyle, Option<&'a InlineStyle>);

pub(super) fn fingerprint(
    pair: Pair<'_>,
    state: &impl BuildHasher,
    limit: Option<u64>,
) -> Result<(u64, u64), LimitExceeded> {
    struct Sink<H> {
        hash: H,
        bytes: u64,
        limit: Option<u64>,
    }
    impl<H: Hasher> Write for Sink<H> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.bytes = self.bytes.saturating_add(s.len() as u64);
            if self.limit.is_some_and(|limit| self.bytes > limit) {
                return Err(fmt::Error);
            }
            self.hash.write(s.as_bytes());
            Ok(())
        }
    }
    let mut sink = Sink {
        hash: state.build_hasher(),
        bytes: 0,
        limit,
    };
    if write!(sink, "{pair:?}").is_err() {
        return Err(LimitExceeded {
            kind: LimitKind::StyleBytes,
            limit: limit.expect("bounded sink"),
            actual: sink.bytes,
        });
    }
    Limits::check(limit, LimitKind::StyleBytes, sink.bytes)?;
    Ok((sink.hash.finish(), sink.bytes))
}

pub(super) fn matches(pair: Pair<'_>, key: &str) -> bool {
    struct Sink<'a>(&'a str);
    impl Write for Sink<'_> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.0 = self.0.strip_prefix(s).ok_or(fmt::Error)?;
            Ok(())
        }
    }
    let mut sink = Sink(key);
    write!(sink, "{pair:?}").is_ok() && sink.0.is_empty()
}

pub(super) fn allocate(pair: Pair<'_>, bytes: u64) -> String {
    let mut key = String::with_capacity(bytes as usize);
    write!(key, "{pair:?}").expect("String writes cannot fail");
    key
}
