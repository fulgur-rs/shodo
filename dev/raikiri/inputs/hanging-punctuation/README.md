# Original hanging-punctuation WPT inputs

These files are byte-for-byte copies from Web Platform Tests revision
`97ea26e26a2aac3eec7e770650b25e7049ed4a4e`, under
`css/css-text/hanging-punctuation/`. The upstream license and attribution are
preserved; Web Platform Tests uses the [3-clause BSD license](https://github.com/web-platform-tests/wpt/blob/97ea26e26a2aac3eec7e770650b25e7049ed4a4e/LICENSE.md).

| File | SHA-256 |
| --- | --- |
| hanging-punctuation-last.html | 9a1f7573c815df95191d5118335c219f77472742543798c0e8d0412f7a9ca042 |
| hanging-punctuation-last-whitespace.html | 88bc75b0bd7d1d394d20d9f6ef9b1bca361c6357d8fa201de412f90726fa65d5 |
| hanging-punctuation-first-and-last-together.html | 9521a981499b7567e1bc418e6149064f305789cd450aed005b401333bee666c8 |
| hanging-punctuation-force-end-001.xht | 28adae1b68c8e56837f157261da80c7ea8c8fb27b8b67791ba6df584bf3c6476 |
| hanging-punctuation-allow-end-001.xht | 6bddf4b8e0779358c005b16f3eb0e34322f70bef2e39c6a03a961e1bdf9940d5 |
| fonts/ahem.css | 5d8b9526d7be573871022125d5ec44f4893e4d851c26ab3fb6df44219422111c |
| fonts/Ahem.ttf | b719ecb31c5b21fc573c03f6421c74ac63c271a5a3ff841e34f9705fb94b8448 |

The caller tests verify the original CSS grammar, computed inheritance and
hanging-punctuation projection. They do not compare painted pages, supply
missing fonts or resources, change reference files, or claim WPT PASS results.
The native IFC assignment is also exercised with the original Ahem resource.
The Japanese IPA fonts requested by the force/allow cases are absent here, so
these checks do not establish original-font geometry. Other unsupported
source-replay fields remain errors. `LICENSE.md` contains the upstream license.
