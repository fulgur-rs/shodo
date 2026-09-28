# Complex-script transformation diagnostics

shodo-0ce originally suspects missing LineSegmenter::new_auto data. Reproduce
Japanese/Khmer text with content_locale and record actual UTF-8 boundaries and
model diagnostics before changing providers. The existing compiled_data+auto
configuration already supplies models: Japanese line cuts are
[0,3,6,9,12,15,18,21]; Khmer ភាសាខ្មែរជាភាសាជាតិ cuts are
[0,12,27,33,57]. Both emit zero model diagnostics. Non-complex Khmer has
[0,57] and emits one diagnostic.

The real ParagraphBuilder Uppercase path emits one diagnostic per sample;
plain paragraphs emit none. transform_context always selects a non-complex
WordSegmenter, even when complex-scripts is enabled. Use WordSegmenter::new_auto
under that feature; retain existing behavior without it. No dependency, public
API, provider, line-analysis, or raikiri spike changes are required.

Regressions must exercise real transformed paragraphs in a subprocess, checking
unchanged uncased text and absence of model errors. Check literal context word
heads independently of stderr logging (Japanese 0/15, Thai 0/12, Khmer
0/12/27/33); this guards the input to capitalization even when diagnostics are
compiled out. Characterize LineSegmenter content_locale using the literals above.
All positions are UTF-8 bytes. Non-complex mode remains intentionally limited.

Keep both unowned raikiri spikes and the independently owned root untouched.
Run workspace/default/AccessKit, no-default/complex, release, fmt/clippy/docs and
allocator gates; independent read-only review; same-head CI before merge.
