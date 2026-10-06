# Horizontal layout contract coverage

This document summarizes horizontal layout behavior and its regression coverage.
The following tests run in `cargo test --workspace`. Fixture tests use the
checked-in Latin/CJK/Arabic faces; library tests also exercise deterministic stub
metrics and independent synthetic font tables. These tests cover layout and
source contracts, rather than claiming pixel equality with a particular renderer.

Test references use `module::test_name`. Public API tests are in
[`crates/shodo/tests/`](../../crates/shodo/tests/), fixed-font tests in
[`dev/harness/tests/`](../../dev/harness/tests/), and library module tests in
[`crates/shodo/src/`](../../crates/shodo/src/).

| Contract | Regression evidence |
| --- | --- |
| Pure `next_line`, paragraph-owned tokens, alternate constraints, accepted output immutability | `horizontal_contracts::real_font_next_line_is_pure_and_height_trials_do_not_consume_source`; `lines::tokens_are_tied_to_their_paragraph`, `a_saved_token_resumes_at_another_width`; `horizontal_contracts::actual_font_alignment_last_line_and_indent_cover_every_value` |
| Float lookahead uses ordinary breaks; first unbreakable word overflows and reports its floats | `floats::word_float_is_not_reported_before_its_word_fits`; `horizontal_contracts::actual_font_float_page_trials_restore_reports_and_geometry`; `line_shaping::real_font_soft_break_precedes_transparent_float` |
| Ordered float withdrawal, source cursor restoration, tab retry, source position inside shared clusters | `floats::withdrawal_is_ordered_and_cursor_survives_line_boundaries`, `tab_retry_defers_float_until_anchor_line_and_withdraws_one_at_a_time`; `spacing::cached_float_retries_match_cold_spacing_and_tab_offsets`; `line_shaping::internal_float_uses_source_line_tokens_and_cached_continuation` |
| Page-height rejection is a pure trial; caller restores float state | `horizontal_contracts::actual_font_float_page_trials_restore_reports_and_geometry`; `floats::page_trial_restores_float_reports_and_replayed_line`; `line_box::height_limit_is_pure_and_can_be_retried`, `atomic_height_retry_and_zero_height_block_prefix` |
| Empty/block boundaries, strut participation, missing atomic dimensions, first-line termination | `line_box::empty_before_block_has_zero_height_but_edges_are_content`, `forced_empty_line_keeps_strut_and_direct_block_is_not_a_line`; `horizontal_contracts::real_font_empty_block_prefix_and_missing_atomic_keep_output_contracts`; `fragments::missing_atomic_sizes_warn_and_collapse_to_zero`; `first_line::block_in_inline_disables_first_line`, `forced_break_does_not_restart_first_line_style`; `iteration::iterator_handles_retry_results_internally_and_yields_boundaries` |
| Actual selected-font strut, mixed fallback, size-adjust, all vertical-align values, deep ancestry | All `line_metrics` fixture tests; `font::metrics` variation tests; `line_box` nested/top/bottom, atomic baseline and padding tests |
| Root font decoration metrics and output font instance metadata | `line_metrics::parent_x_height_drives_middle_and_text_edges`; `shaping::synthesis_size_adjust_and_run_instance_are_public`, `every_size_adjust_metric_reaches_public_font_size`; `font::metrics::tests::variation_and_decoration_metrics_match_run_instance` |
| All alignment and last-line values, justify-all, hanging/each-line indent, preserved hanging whitespace | `horizontal_contracts::actual_font_alignment_last_line_and_indent_cover_every_value`, `final_line_metrics_and_ink_overflow_are_explicit`; `alignment::rtl_physical_alignment_and_no_justification`, `justification_excludes_hanging_spaces_and_preserves_other_lines`; `final_review::opposite_plaintext_direction_centers_and_justifies_inside_available_width` |
| Unexpandable justified lines and typographic boundaries within retained/owned ligatures | `horizontal_contracts::unexpandable_justification_uses_last_alignment_and_direction`, `retained_ligatures_justify_each_legal_typographic_boundary`, `justification_does_not_open_indivisible_transforms_or_empty_lines`; independent expected positions for ffi/space/x in `line_shaping::unbroken_ligature_slices_do_not_overwrite_justified_glyph_positions` |
| Root space/ch tab stops, root versus inline style, offsets, minimum-stop distance, zero tab interval | `spacing::word_spacing_and_tabs_use_real_space_metrics`, `tabs_use_block_space_and_skip_too_close_stops`, `cached_float_retries_match_cold_spacing_and_tab_offsets`; `hit_test::tab_carets_use_final_layout_slots_in_both_directions` |
| Signed tracking, word spacing, visual neighbor halves, marks, cursive fallback, retained ligatures | `spacing` fixture tests; independent expected ordering and bounded prefix work in `line::spacing_summary` |
| Autospace classification, visual adjacency, containing inline's ic, box edge barriers, parent ownership, first-line | Autospace `spacing` fixture tests; independent expected ancestry and ordering in `line::autospace`; `line::windows::mixed_width_kana_whole_cluster_uses_each_source_box_for_autospace` forces cross-node mixed-width GSUB and zero-reshape fallback |
| Min/max content, floats and first-line contributions, contextual shaping and soft-hyphen cost, Balance/Pretty | `intrinsic` library tests; `first_line` intrinsic fixture cases; `spacing::spacing_break_candidates_and_plans_use_actual_geometry`, `autospace_removed_at_soft_wrap_and_intrinsics`, `visible_soft_hyphen_tracking_reaches_intrinsics_and_plans`; `line_shaping` intrinsic and planned-line tests |
| Glyph id/positions/advance, shared versus owned runs, font data lifetime, processed source and cluster flags | `analysis_limits::all_corpus_public_glyph_ids_have_font_data`; `shaping::font_layer_survives_owner_drop`; `line_shaping` shared-ligature, soft-hyphen, and owned-overlay tests; `horizontal_contracts::cluster_flags_preserve_source_and_discretionary_hyphens`; `hit_test::cross_line_selection_and_navigation_survive_owner_drop` |
| Final line metrics and nominal ink/paint bounds distinct from line advance | `horizontal_contracts::final_line_metrics_and_ink_overflow_are_explicit` compares bounds directly to Skrifa; `signed_atomic_margins_do_not_make_negative_caret_height` pins painted atomic height despite negative margin-box height |
| Hit/caret, double affinity, GDEF variation/fallback, legal grapheme and indivisible transform cuts | `hit_test` fixture tests; `hit::index::gdef_carets_scale_and_apply_actual_variation_with_safe_fallbacks` |
| Discontiguous selection, partial ligature, reversed endpoints, collapsed/expanded and first-line datasets, cross-line navigation | `hit_test` fixture tests |
| Nonfinite/negative/extreme input normalization and source progress | `horizontal_contracts::nonfinite_and_extreme_styles_keep_geometry_finite_and_source_progressing`; `normalization`, `build`, `lines` adverse inputs; generated properties in `properties` |
| Tiny/zero reshape and aggregate glyph/text budgets | `line_shaping::unavailable_hyphen_window_keeps_word_whole_in_greedy_intrinsics_and_plans`, `too_small_ligature_slice_window_retains_whole_cluster_and_warns`; `first_line` aggregate cases; `analysis_limits`, `processed_limits`, `line::windows` synthetic-budget cases |
| Bounded deep/long bidi, styles, controls, cursor and per-query work | `line::spacing_summary::long_alternating_prefixes_keep_bounded_work_and_memory`; `line::autospace::deep_inline_index_keeps_logarithmic_ancestor_queries`; `line::spacing::tests::many_styles_are_not_rescanned_for_every_line`; `hit::spatial::repeated_coordinate_queries_do_not_scan_all_characters_or_lines`; `hit::tests::repeated_hits_and_navigation_do_not_rebuild_glyph_indexes`; `properties` library tests and `final_review` fixture tests |

Line coordinates are logical. Glyph/fragment block coordinates and nominal
overflow bounds are line-local; hit, caret and selection block coordinates
include `Line::block_offset`. The caller converts logical axes for painting.
Nominal overflow does not include renderer-added strokes, antialiasing or
decoration effects. Japanese punctuation spacing and hanging are described in
the [Japanese layout contract](japanese-layout.md); vertical typography is
described in the [vertical output contract](vertical-layout.md). [Ruby layout](ruby.md)
adds coordinated annotation lanes and ink overflow. Emphasis placement belongs
to the renderer.

## Quirks-mode line height

`ParagraphStyle::line_height_quirk` implements the line height calculation
quirk of quirks and limited-quirks documents. It is decided per line and per
inline box, the root inline box included. A box contributes its strut to a
line only when, on that line:

- it directly contains text or preserved white space (collapsible spaces
  removed at the line end do not count; hidden soft hyphens and other
  zero-width characters do);
- its own inline-start border or padding (on the line holding its start) or
  inline-end border or padding (on the line holding its end) is nonzero —
  margins never count;
- it holds a forced break and has no metrics credited by earlier content
  in its on-line subtree. Text, atomic inlines and inline border/padding
  edges credit ancestors without an intervening `top`/`bottom` box. Empty
  `text-top`/`text-bottom` children also credit their parent; `top`/`bottom`
  children credit the nearest `top`/`bottom` ancestor or the root, even when
  empty. Root and `top`/`bottom` boxes receive any such subtree credit;
- it is the root inline box and the line holds ruby.

A box that does not contribute is ignored only for sizing the line box;
descendants still align to its font metrics. Matching Chromium, a
`box-decoration-break: clone` edge repeated on a continuation line does not
count, although CSS Inline 3 §5.3 speaks of fragments. The forced-break
credit follows Blink's pending vertical-align handling (shodo-9kt): a
`top`/`bottom` child does not prevent a baseline ancestor's break strut,
whereas an empty `text-top`/`text-bottom` child does. Empty immediate shifts
(`sub`, `super`, `middle`, lengths) do not credit ancestors. List-item lines
do not force the root strut (shodo-qu8).
