//! Hash maps for internal, index-keyed bookkeeping. Keys are never
//! attacker-chosen names and the maps are short-lived or bounded, so the
//! default SipHash's DoS resistance buys nothing; foldhash with a fixed seed
//! hashes several times faster and keeps iteration order deterministic.
pub(crate) type FastMap<K, V> = std::collections::HashMap<K, V, foldhash::fast::FixedState>;
pub(crate) type FastSet<T> = std::collections::HashSet<T, foldhash::fast::FixedState>;
