# Parley complex-scripts comparison

## Scope

This records the isolated comparison for shodo-bqz. It used the issue's pinned
RAiKiri, S4, and WPT revisions, the saved WPT selection, and the same fonts and
Cargo locks in both builds. Only Parley's complex-scripts feature changed.
The Shodo candidate kept the same complex-scripts configuration in both runs.

The checker exercised retained-layout width and height retry paths on the 23
diagnostic WPT documents. It did not run browser reftests, measure performance,
or validate complete-page pagination.

## Results

- With Parley complex-scripts disabled, the native-initial phase emitted 75
  ICU4X missing-segmentation-model diagnostics across all 23 selected documents:
  47 for Chinese/Japanese and 28 for Thai. With the feature enabled, it emitted
  zero.
- The checker produced 375 rows in each run. All 186 Shodo candidate rows were
  identical. Of 189 native Parley rows, 181 were identical and 8 changed.
- Every changed native row was a Thai auto-phrase fallback test or its reference
  at a forced width of 384px. The disabled build kept the 48-byte text range on
  one line with 16 glyphs and 448px advance. The enabled build broke it into
  source ranges 0–30 and 30–48, with 10 and 6 glyphs and advances of 256px and
  192px. Both lines were 32px high; their baselines were 28px and 60px. The
  glyph ID sequence, font hash, and glyph advances were unchanged; line ranges,
  line count, and vertical positions changed.
- The native height-rejection count rose from 363 to 371 because the enabled
  layout exposed a second line in these cases. The Shodo candidate count stayed
  at 374. All 46 unsupported adapter diagnostics were identical between runs.

The native output hashes cover line ranges, metrics, source ranges, font hashes,
glyph IDs, positions, and advances. An inspection-only copy of the checker
recorded the representative snapshots; it did not modify the checked-in probe
or either saved spike.

## Recommendation and limits

Enable Parley's complex-scripts feature, or provide equivalent segmentation
model data, when RAiKiri's native layout is used as the reference for complex
scripts. In this sample, disabling the feature produced missing-model
diagnostics and left Thai fallback text wider than the tested line width.

This establishes a need for the feature in the native comparison path for the
affected inputs. It does not establish that Shodo needs a production feature
switch: Shodo candidate output was unchanged, and this run did not perform WPT
image reftests or a production-path comparison. No performance result was
collected. The saved S4 feature baseline and original inputs remain unchanged;
this evidence does not add a mandatory shodo-p2m.6 dependency.

Machine-readable counts, row hashes, source hashes, and build provenance are in
[the experiment data](data/raikiri-bqz-complex-scripts.json).
