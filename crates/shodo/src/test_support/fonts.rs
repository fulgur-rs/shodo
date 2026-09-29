//! Fixed fonts for `src/` unit tests, loaded from the workspace-only
//! `dev/fixtures` directory (not part of the published `shodo` package;
//! see the `[package].include` allowlist in `Cargo.toml`).

pub(crate) const LATIN: &[u8] = include_bytes!("../../../../dev/fixtures/assets/fonts/latin.ttf");
pub(crate) const CJK: &[u8] = include_bytes!("../../../../dev/fixtures/assets/fonts/cjk.otf");
pub(crate) const ARABIC: &[u8] = include_bytes!("../../../../dev/fixtures/assets/fonts/arabic.ttf");
pub(crate) const EMOJI_COLOR: &[u8] =
    include_bytes!("../../../../dev/fixtures/assets/fonts/emoji-color.ttf");
