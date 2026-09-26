Primary language subtags from the IANA Language Subtag Registry.

Source: https://www.iana.org/assignments/language-subtag-registry/language-subtag-registry
File-Date: 2026-09-17
SHA-256 of source: 755fad43283be7b41ebe3c89ad054b6eaf928f404f9c0edb74799e0eab74beb1

`languages.dat` stores sorted fixed-width three-byte records, padding
 two-letter subtags with a space. Deprecated subtags remain recognized.
The reserved private-use primary range qaa–qtz is recognized separately.
Only language identifiers are retained; descriptions are not distributed.

Regenerate by selecting records with Type: language, excluding the
qaa..qtz range, padding each Subtag to three ASCII bytes, sorting, and
concatenating without separators. No runtime network dependency.
